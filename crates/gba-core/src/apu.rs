//! Clock-driven PSG and Direct Sound. See docs/AUDIO.md for accuracy limits.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Stereo PCM output rate in frames per second (GBA master clock / 512).
pub const AUDIO_SAMPLE_RATE: u32 = 32768;
const QUEUE_LIMIT: usize = AUDIO_SAMPLE_RATE as usize / 2;

mod bounded_fifo {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::VecDeque;

    pub fn serialize<S>(value: &[VecDeque<i8>; 2], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[VecDeque<i8>; 2], D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct FifoPair(
            #[serde(deserialize_with = "bounded_deque")] VecDeque<i8>,
            #[serde(deserialize_with = "bounded_deque")] VecDeque<i8>,
        );

        fn bounded_deque<'de, D>(deserializer: D) -> Result<VecDeque<i8>, D::Error>
        where
            D: Deserializer<'de>,
        {
            crate::state::deserialize_bounded_deque(deserializer)
        }

        let FifoPair(first, second) = FifoPair::deserialize(deserializer)?;
        Ok([first, second])
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Channel {
    enabled: bool,
    length: u16,
    phase: usize,
    clocks: u64,
    volume: u8,
    envelope: u8,
}

fn default_periods() -> [u64; 4] {
    [32768, 32768, 16384, 32]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Apu {
    channels: [Channel; 4],
    // Derived from sound registers; do not change the version-1 state payload.
    // Bus restoration rebuilds these after restoring the separate I/O bytes.
    #[serde(skip, default = "default_periods")]
    periods: [u64; 4],
    wave: [[u8; 16]; 2],
    wave_bank: usize,
    noise: u16,
    sweep_frequency: u16,
    sweep_ticks: u8,
    sequencer_clocks: u64,
    sequencer_step: u8,
    sample_clocks: u64,
    #[serde(with = "bounded_fifo")]
    fifo: [VecDeque<i8>; 2],
    held: [i8; 2],
    #[serde(deserialize_with = "crate::state::deserialize_bounded_deque")]
    samples: VecDeque<[i16; 2]>,
    pub(crate) generated: u64,
    pub(crate) dropped: u64,
}

impl Default for Apu {
    fn default() -> Self {
        Self {
            channels: std::array::from_fn(|_| Channel::default()),
            periods: default_periods(),
            wave: [[0; 16]; 2],
            wave_bank: 0,
            noise: 0x4000,
            sweep_frequency: 0,
            sweep_ticks: 0,
            sequencer_clocks: 0,
            sequencer_step: 0,
            sample_clocks: 0,
            fifo: std::array::from_fn(|_| VecDeque::with_capacity(32)),
            held: [0; 2],
            samples: VecDeque::new(),
            generated: 0,
            dropped: 0,
        }
    }
}

fn half(registers: &[u8; 0x400], offset: usize) -> u16 {
    u16::from_le_bytes([registers[offset], registers[offset + 1]])
}

impl Apu {
    fn channel_period(index: usize, registers: &[u8; 0x400]) -> u64 {
        if index == 3 {
            let value = registers[0x7c];
            (if value & 7 == 0 {
                32
            } else {
                64 * u64::from(value & 7)
            }) << (value >> 4)
        } else {
            let frequency = half(registers, [0x64, 0x6c, 0x74][index]) & 2047;
            u64::from(2048 - frequency) * if index == 2 { 8 } else { 16 }
        }
    }

    pub(crate) fn rebuild_periods(&mut self, registers: &[u8; 0x400]) {
        self.periods = std::array::from_fn(|index| Self::channel_period(index, registers));
    }

    pub(crate) fn valid_snapshot(&self) -> bool {
        self.wave_bank < 2
            && self.fifo.iter().all(|fifo| fifo.len() <= 32)
            && self.samples.len() <= QUEUE_LIMIT
            && self.sample_clocks < 512
            && self.sequencer_step < 8
    }

    pub(crate) fn next_sample(&self) -> u64 {
        512 - self.sample_clocks
    }

    pub(crate) fn take_samples(&mut self) -> Vec<[i16; 2]> {
        self.samples.drain(..).collect()
    }

    pub(crate) fn fifo_len(&self, channel: usize) -> usize {
        self.fifo[channel].len()
    }

    pub(crate) fn clock_fifo(&mut self, channel: usize, count: u64) {
        for _ in 0..count.min(33) {
            self.held[channel] = self.fifo[channel].pop_front().unwrap_or(0);
        }
    }

    pub(crate) fn read(&self, offset: usize, registers: &[u8; 0x400]) -> u8 {
        if (0x90..0xa0).contains(&offset) {
            return self.wave[self.wave_bank ^ 1][offset - 0x90];
        }
        if offset == 0x84 {
            return (registers[0x84] & 0x80)
                | self
                    .channels
                    .iter()
                    .enumerate()
                    .fold(0, |bits, (index, channel)| {
                        bits | (u8::from(channel.enabled) << index)
                    });
        }
        let mask = match offset {
            0x60 => 0x7f,
            0x62 | 0x68 => 0xc0,
            0x63 | 0x69 | 0x79 | 0x7c => 0xff,
            0x73 => 0xe0,
            0x65 | 0x6d | 0x75 | 0x7d => 0x40,
            0x70 => 0xe0,
            0x80 => 0x77,
            0x81 => 0xff,
            0x82 => 0x0f,
            0x83 => 0x77,
            0x88 => 0xfe,
            0x89 => 0xc3,
            _ => 0,
        };
        registers[offset] & mask
    }

    pub(crate) fn write(&mut self, offset: usize, value: u8, registers: &mut [u8; 0x400]) {
        if (0x90..0xa0).contains(&offset) {
            self.wave[self.wave_bank ^ 1][offset - 0x90] = value;
            return;
        }
        if (0xa0..0xa8).contains(&offset) {
            let fifo = (offset - 0xa0) / 4;
            if self.fifo[fifo].len() < 32 {
                self.fifo[fifo].push_back(value as i8);
            }
            return;
        }
        if offset == 0x84 {
            if value & 0x80 == 0 {
                registers[0x60..0x82].fill(0);
                self.channels = std::array::from_fn(|_| Channel::default());
                self.periods = default_periods();
                self.sequencer_clocks = 0;
                self.sequencer_step = 0;
                self.wave_bank = 0;
            }
            registers[offset] = value & 0x80;
            return;
        }
        if offset < 0x82 && registers[0x84] & 0x80 == 0 {
            return;
        }
        if offset == 0x83 {
            for channel in 0..2 {
                if value & (8 << (channel * 4)) != 0 {
                    self.fifo[channel].clear();
                    self.held[channel] = 0;
                }
            }
            registers[offset] = value & 0x77;
            return;
        }
        registers[offset] = value;
        let period_channel = match offset {
            0x64 | 0x65 => Some(0),
            0x6c | 0x6d => Some(1),
            0x74 | 0x75 => Some(2),
            0x7c => Some(3),
            _ => None,
        };
        if let Some(index) = period_channel {
            self.periods[index] = Self::channel_period(index, registers);
        }
        if offset == 0x70 {
            self.wave_bank = usize::from(value >> 6 & 1);
            if value & 0x80 == 0 {
                self.channels[2].enabled = false;
            }
        }
        if let Some(index) = [0x62, 0x68, 0x72, 0x78]
            .iter()
            .position(|base| *base == offset)
        {
            self.channels[index].length = if index == 2 {
                256 - u16::from(value)
            } else {
                64 - u16::from(value & 63)
            };
        }
        if let Some(index) = [0x63, 0x69, 0x79].iter().position(|base| *base == offset)
            && value & 0xf8 == 0
        {
            self.channels[[0, 1, 3][index]].enabled = false;
        }
        if let Some(index) = [0x65, 0x6d, 0x75, 0x7d]
            .iter()
            .position(|base| *base == offset)
            && value & 0x80 != 0
        {
            let envelope = registers[[0x63, 0x69, 0x73, 0x79][index]];
            let channel = &mut self.channels[index];
            channel.enabled = if index == 2 {
                registers[0x70] & 0x80 != 0
            } else {
                envelope & 0xf8 != 0
            };
            if channel.length == 0 {
                channel.length = if index == 2 { 256 } else { 64 };
            }
            channel.volume = envelope >> 4;
            channel.envelope = (envelope & 7).max(1);
            channel.phase = 0;
            channel.clocks = 0;
            if index == 2 {
                self.wave_bank = usize::from(registers[0x70] >> 6 & 1);
            }
            if index == 3 {
                self.noise = if registers[0x7c] & 8 != 0 {
                    0x40
                } else {
                    0x4000
                };
            }
            if index == 0 {
                self.sweep_frequency = half(registers, 0x64) & 2047;
                self.sweep_ticks = ((registers[0x60] >> 4) & 7).max(1);
                if registers[0x60] & 7 != 0 && self.sweep_value(registers[0x60]).is_none() {
                    self.channels[0].enabled = false;
                }
            }
            registers[offset] &= 0x7f;
        }
    }

    fn sweep_value(&self, control: u8) -> Option<u16> {
        let delta = self.sweep_frequency >> (control & 7);
        let next = if control & 8 != 0 {
            self.sweep_frequency.checked_sub(delta)?
        } else {
            self.sweep_frequency + delta
        };
        (next <= 2047).then_some(next)
    }

    fn sequence(&mut self, registers: &mut [u8; 0x400]) {
        if self.sequencer_step.is_multiple_of(2) {
            for (index, channel) in self.channels.iter_mut().enumerate() {
                if registers[[0x65, 0x6d, 0x75, 0x7d][index]] & 0x40 != 0 && channel.length > 0 {
                    channel.length -= 1;
                    if channel.length == 0 {
                        channel.enabled = false;
                    }
                }
            }
        }
        if matches!(self.sequencer_step, 2 | 6) && self.channels[0].enabled {
            self.sweep_ticks = self.sweep_ticks.saturating_sub(1);
            if self.sweep_ticks == 0 {
                let control = registers[0x60];
                self.sweep_ticks = ((control >> 4) & 7).max(1);
                if control & 0x70 != 0 && control & 7 != 0 {
                    if let Some(next) = self.sweep_value(control) {
                        self.sweep_frequency = next;
                        registers[0x64] = next as u8;
                        registers[0x65] = (registers[0x65] & 0x40) | (next >> 8) as u8;
                        self.periods[0] = Self::channel_period(0, registers);
                        if self.sweep_value(control).is_none() {
                            self.channels[0].enabled = false;
                        }
                    } else {
                        self.channels[0].enabled = false;
                    }
                }
            }
        }
        if self.sequencer_step == 7 {
            for index in [0, 1, 3] {
                let envelope = registers[[0x63, 0x69, 0x73, 0x79][index]];
                let channel = &mut self.channels[index];
                if envelope & 7 == 0 {
                    continue;
                }
                channel.envelope = channel.envelope.saturating_sub(1);
                if channel.envelope == 0 {
                    channel.envelope = envelope & 7;
                    if envelope & 8 != 0 {
                        channel.volume = (channel.volume + 1).min(15);
                    } else {
                        channel.volume = channel.volume.saturating_sub(1);
                    }
                }
            }
        }
        self.sequencer_step = (self.sequencer_step + 1) & 7;
    }

    pub(crate) fn advance(&mut self, cycles: u64, registers: &mut [u8; 0x400]) {
        // Bus slices advancement at PCM boundaries and timer overflow edges.
        for index in 0..4 {
            if !self.channels[index].enabled {
                continue;
            }
            let period = self.periods[index];
            let channel = &mut self.channels[index];
            channel.clocks += cycles;
            while channel.clocks >= period {
                channel.clocks -= period;
                if index == 3 {
                    let carry = self.noise & 1;
                    self.noise >>= 1;
                    if carry != 0 {
                        self.noise ^= if registers[0x7c] & 8 != 0 {
                            0x60
                        } else {
                            0x6000
                        };
                    }
                } else {
                    channel.phase += 1;
                    if index == 2 && channel.phase == 32 {
                        channel.phase = 0;
                        if registers[0x70] & 0x20 != 0 {
                            self.wave_bank ^= 1;
                        }
                    } else if index != 2 {
                        channel.phase &= 7;
                    }
                }
            }
        }
        if registers[0x84] & 0x80 != 0 {
            self.sequencer_clocks += cycles;
        }
        if self.sequencer_clocks >= 32768 {
            self.sequencer_clocks -= 32768;
            self.sequence(registers);
        }
        self.sample_clocks += cycles;
        if self.sample_clocks == 512 {
            self.sample_clocks = 0;
            if self.samples.len() == QUEUE_LIMIT {
                self.samples.pop_front();
                self.dropped += 1;
            }
            self.samples.push_back(self.mix(registers));
            self.generated += 1;
        }
    }

    fn mix(&self, registers: &[u8; 0x400]) -> [i16; 2] {
        if registers[0x84] & 0x80 == 0 {
            return [0, 0];
        }
        let mut levels = [0i32; 4];
        const DUTY: [u8; 4] = [0b00000001, 0b10000001, 0b10000111, 0b01111110];
        for (index, channel) in self.channels.iter().enumerate() {
            if !channel.enabled {
                continue;
            }
            let amplitude = match index {
                0 | 1 => {
                    if DUTY[usize::from(registers[if index == 0 { 0x62 } else { 0x68 }] >> 6)]
                        & (1 << channel.phase)
                        != 0
                    {
                        i32::from(channel.volume)
                    } else {
                        -i32::from(channel.volume)
                    }
                }
                2 => {
                    let byte = self.wave[self.wave_bank][channel.phase / 2];
                    let value = i32::from(if channel.phase & 1 == 0 {
                        byte >> 4
                    } else {
                        byte & 15
                    });
                    let value = value * 2 - 15;
                    let volume = registers[0x73];
                    if volume & 0x80 != 0 {
                        value * 3 / 4
                    } else {
                        match volume >> 5 & 3 {
                            0 => 0,
                            1 => value,
                            2 => value / 2,
                            _ => value / 4,
                        }
                    }
                }
                _ => {
                    if self.noise & 1 == 0 {
                        i32::from(channel.volume)
                    } else {
                        -i32::from(channel.volume)
                    }
                }
            };
            levels[index] = amplitude;
        }
        let control = half(registers, 0x80);
        let mix = half(registers, 0x82);
        let bias = i32::from(half(registers, 0x88) & 0x3fe);
        let shift = 1 + (registers[0x89] >> 6);
        std::array::from_fn(|side| {
            // API order is left/right; register routing lists right before left.
            let psg_mask = if side == 0 { 12 } else { 8 };
            let volume = if side == 0 {
                (control >> 4) & 7
            } else {
                control & 7
            };
            let mut value = 0;
            for (index, level) in levels.iter().enumerate() {
                if control & (1 << (psg_mask + index)) != 0 {
                    value += level * i32::from(volume + 1);
                }
            }
            value >>= [2, 1, 0, 0][usize::from(mix & 3)];
            for fifo in 0..2 {
                if mix & (1 << (8 + fifo * 4 + usize::from(side == 0))) != 0 {
                    value += i32::from(self.held[fifo])
                        * if mix & (1 << (2 + fifo)) != 0 { 4 } else { 2 };
                }
            }
            let dac = ((value + bias).clamp(0, 1023) >> shift) << shift;
            ((dac - 512) * 64) as i16
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Apu;
    fn registers() -> [u8; 0x400] {
        let mut r = [0; 0x400];
        r[0x84] = 0x80;
        r[0x89] = 2;
        r
    }
    // Original advancement path: decode periods from registers on every call.
    // Sequencing and mixing are shared, but this never consults cached periods.
    fn original_advance(apu: &mut Apu, cycles: u64, registers: &mut [u8; 0x400]) {
        for index in 0..4 {
            if !apu.channels[index].enabled {
                continue;
            }
            let frequency = super::half(registers, [0x64, 0x6c, 0x74, 0x7c][index]) & 2047;
            let period = if index == 3 {
                let value = registers[0x7c];
                (if value & 7 == 0 {
                    32
                } else {
                    64 * u64::from(value & 7)
                }) << (value >> 4)
            } else {
                u64::from(2048 - frequency) * if index == 2 { 8 } else { 16 }
            };
            let channel = &mut apu.channels[index];
            channel.clocks += cycles;
            while channel.clocks >= period {
                channel.clocks -= period;
                if index == 3 {
                    let carry = apu.noise & 1;
                    apu.noise >>= 1;
                    if carry != 0 {
                        apu.noise ^= if registers[0x7c] & 8 != 0 {
                            0x60
                        } else {
                            0x6000
                        };
                    }
                } else {
                    channel.phase += 1;
                    if index == 2 && channel.phase == 32 {
                        channel.phase = 0;
                        if registers[0x70] & 0x20 != 0 {
                            apu.wave_bank ^= 1;
                        }
                    } else if index != 2 {
                        channel.phase &= 7;
                    }
                }
            }
        }
        if registers[0x84] & 0x80 != 0 {
            apu.sequencer_clocks += cycles;
        }
        if apu.sequencer_clocks >= 32768 {
            apu.sequencer_clocks -= 32768;
            apu.sequence(registers);
        }
        apu.sample_clocks += cycles;
        if apu.sample_clocks == 512 {
            apu.sample_clocks = 0;
            if apu.samples.len() == super::QUEUE_LIMIT {
                apu.samples.pop_front();
                apu.dropped += 1;
            }
            apu.samples.push_back(apu.mix(registers));
            apu.generated += 1;
        }
    }

    #[test]
    fn cached_periods_cover_frequency_and_noise_register_values() {
        let mut apu = Apu::default();
        let mut r = registers();
        for (channel, offset) in [0x64, 0x6c, 0x74].into_iter().enumerate() {
            for frequency in 0..2048u16 {
                apu.write(offset, frequency as u8, &mut r);
                let intermediate = super::half(&r, offset) & 2047;
                let scale = if channel == 2 { 8 } else { 16 };
                assert_eq!(apu.periods[channel], u64::from(2048 - intermediate) * scale);
                apu.write(offset + 1, (frequency >> 8) as u8, &mut r);
                assert_eq!(apu.periods[channel], u64::from(2048 - frequency) * scale);
            }
        }
        for value in 0..=255u8 {
            apu.write(0x7c, value, &mut r);
            let divisor = [32u64, 64, 128, 192, 256, 320, 384, 448][(value & 7) as usize];
            assert_eq!(apu.periods[3], divisor * (1u64 << (value >> 4)));
        }
        apu.write(0x84, 0, &mut r);
        assert_eq!(apu.periods, super::default_periods());
        for offset in [0x64, 0x65, 0x6c, 0x6d, 0x74, 0x75, 0x7c] {
            apu.write(offset, 0xff, &mut r);
        }
        assert_eq!(
            apu.periods,
            super::default_periods(),
            "powered-off writes are ignored"
        );
    }

    #[test]
    fn cached_advancement_matches_original_pcm_and_state() {
        for sweep in [0x11, 0x19] {
            let mut actual = Apu::default();
            let mut expected = actual.clone();
            let mut ra = registers();
            let mut rb = ra;
            let mut seed = 0x8753_1429u32;
            let mut varied_pcm = false;
            for step in 0..8192 {
                let mut writes = Vec::new();
                if step % 1024 == 0 {
                    writes.extend([
                        (0x84, 0),
                        (0x64, 0xff),
                        (0x7c, 0xff), // Reset, then ignored writes.
                        (0x84, 0x80),
                        (0x60, sweep),
                        (0x62, 0x80),
                        (0x63, 0xf2),
                        (0x64, 0),
                        (0x65, 0x82),
                        (0x68, 0x40),
                        (0x69, 0xb3),
                        (0x6c, 0xff),
                        (0x6d, 0x87),
                        (0x70, 0xe0),
                        (0x72, 0),
                        (0x73, 0x20),
                        (0x74, 0xff),
                        (0x75, 0x87),
                        (0x79, 0xf1),
                        (0x7c, 8),
                        (0x7d, 0x80),
                        (0x80, 0x77),
                        (0x81, 0xff),
                        (0x82, 0x0e),
                        (0x83, 0x33),
                        (0x88, 0),
                        (0x89, 2),
                    ]);
                    for offset in 0x90..0xa0 {
                        writes.push((offset, (offset as u8).wrapping_mul(37)));
                    }
                }
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if step % 53 == 0 {
                    let offset = [0x64, 0x6c, 0x74][(step / 53) % 3];
                    writes.extend([(offset, seed as u8), (offset + 1, (seed >> 8) as u8 & 7)]);
                }
                if step % 37 == 0 {
                    writes.push((0x7c, (seed >> 16) as u8));
                }
                if step % 211 == 0 {
                    writes.push(([0x65, 0x6d, 0x75, 0x7d][(step / 211) % 4], 0xc7));
                }
                for (offset, value) in writes {
                    actual.write(offset, value, &mut ra);
                    expected.write(offset, value, &mut rb);
                }
                if step % 7 == 0 {
                    for fifo in 0..2 {
                        actual.write(0xa0 + fifo * 4, seed as u8, &mut ra);
                        expected.write(0xa0 + fifo * 4, seed as u8, &mut rb);
                        actual.clock_fifo(fifo, 1);
                        expected.clock_fifo(fifo, 1);
                    }
                }
                let cycles = (1 + u64::from(seed & 511)).min(actual.next_sample());
                actual.advance(cycles, &mut ra);
                original_advance(&mut expected, cycles, &mut rb);
                assert_eq!(ra, rb, "sweep={sweep}, step={step}");
                for index in 0..4 {
                    assert_eq!(actual.periods[index], Apu::channel_period(index, &ra));
                }
                if step % 256 == 255 {
                    assert_eq!(
                        postcard::to_allocvec(&actual).expect("serialize cached APU"),
                        postcard::to_allocvec(&expected).expect("serialize original APU"),
                        "sweep={sweep}, step={step}"
                    );
                    let pcm = actual.take_samples();
                    assert_eq!(pcm, expected.take_samples());
                    varied_pcm |= pcm.windows(2).any(|frames| frames[0] != frames[1]);
                }
            }
            assert!(
                varied_pcm,
                "differential playback must exercise changing output"
            );
        }
    }

    #[test]
    fn cached_periods_preserve_legacy_serialized_layout() {
        let mut apu = Apu::default();
        let mut r = registers();
        for (offset, value) in [(0x63, 0xf0), (0x64, 0xff), (0x65, 0x87), (0x7c, 0xdb)] {
            apu.write(offset, value, &mut r);
        }
        apu.advance(512, &mut r);
        // Exactly the original APU field sequence, before the derived cache existed.
        let legacy = postcard::to_allocvec(&(
            &apu.channels,
            &apu.wave,
            apu.wave_bank,
            apu.noise,
            apu.sweep_frequency,
            apu.sweep_ticks,
            apu.sequencer_clocks,
            apu.sequencer_step,
            apu.sample_clocks,
            &apu.fifo,
            apu.held,
            &apu.samples,
            apu.generated,
            apu.dropped,
        ))
        .expect("serialize original APU layout");
        assert_eq!(
            postcard::to_allocvec(&apu).expect("serialize cached APU"),
            legacy
        );
        let mut restored: Apu = postcard::from_bytes(&legacy).expect("decode original APU layout");
        restored.rebuild_periods(&r);
        assert_eq!(restored.periods, apu.periods);
    }

    #[test]
    fn cached_periods_follow_positive_and_negative_sweep() {
        for (control, expected_frequency) in [(0x11, 768u16), (0x19, 256)] {
            let mut apu = Apu::default();
            let mut r = registers();
            for (offset, value) in [(0x60, control), (0x63, 0xf0), (0x64, 0), (0x65, 0x82)] {
                apu.write(offset, value, &mut r);
            }
            apu.sequencer_step = 2;
            apu.sequence(&mut r);
            assert_eq!(super::half(&r, 0x64) & 2047, expected_frequency);
            assert_eq!(apu.periods[0], u64::from(2048 - expected_frequency) * 16);
            let mut expected = apu.clone();
            let mut expected_registers = r;
            for _ in 0..128 {
                apu.advance(512, &mut r);
                original_advance(&mut expected, 512, &mut expected_registers);
            }
            assert_eq!(apu.take_samples(), expected.take_samples());
            assert_eq!(r, expected_registers);
        }
    }

    #[test]
    fn muted_psg_channels_do_not_clip_direct_sound_with_dc() {
        let mut a = Apu::default();
        let mut r = registers();
        for channel in &mut a.channels {
            channel.enabled = true;
            channel.volume = 0;
        }
        r[0x80] = 0x77;
        r[0x81] = 0xff;
        r[0x82] = 2;
        assert_eq!(a.mix(&r), [0, 0], "zero envelope/wave volume is silent");
        r[0x82] |= 4;
        r[0x83] = 3;
        a.held[0] = 10;
        assert_eq!(
            a.mix(&r),
            [2560, 2560],
            "quiet PSG must not bias or clip PCM"
        );
    }

    #[test]
    fn partitioning_clock_advancement_preserves_pcm() {
        let mut a = Apu::default();
        let mut ra = registers();
        for (offset, value) in [
            (0x62, 0x80),
            (0x63, 0xf2),
            (0x64, 0x70),
            (0x65, 0x87),
            (0x80, 0x77),
            (0x81, 0x11),
        ] {
            a.write(offset, value, &mut ra);
        }
        let mut b = a.clone();
        let mut rb = ra;
        for _ in 0..1000 {
            a.advance(512, &mut ra);
            for _ in 0..4 {
                b.advance(128, &mut rb);
            }
        }
        let aa = a.take_samples();
        let bb = b.take_samples();
        assert_eq!(aa, bb);
        assert!(aa.iter().any(|frame| frame[0] != 0));
        assert!(aa.windows(2).any(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn noise_sequence_and_sweep_overflow_follow_register_controls() {
        let mut a = Apu::default();
        let mut r = registers();
        a.write(0x79, 0xf0, &mut r);
        a.write(0x7c, 8, &mut r);
        a.write(0x7d, 0x80, &mut r);
        for _ in 0..7 {
            a.advance(32, &mut r);
        }
        assert_eq!(a.noise, 0x60);
        a.write(0x60, 0x11, &mut r);
        a.write(0x63, 0xf0, &mut r);
        a.write(0x64, 0xff, &mut r);
        a.write(0x65, 0x87, &mut r);
        assert!(
            !a.channels[0].enabled,
            "sweep overflow on trigger disables square 1"
        );
    }

    #[test]
    fn fifo_is_signed_ordered_bounded_and_resettable() {
        let mut apu = Apu::default();
        let mut r = registers();
        for value in 0..40 {
            apu.write(0xa0, value, &mut r);
        }
        assert_eq!(apu.fifo_len(0), 32);
        apu.clock_fifo(0, 1);
        assert_eq!(apu.held[0], 0);
        apu.clock_fifo(0, 1);
        assert_eq!(apu.held[0], 1);
        apu.write(0x83, 8, &mut r);
        assert_eq!(apu.fifo_len(0), 0);
        apu.write(0xa0, 0x80, &mut r);
        apu.clock_fifo(0, 1);
        assert_eq!(apu.held[0], -128);
        r[0x82] = 4;
        r[0x83] = 2;
        assert_eq!(
            apu.mix(&r),
            [-32768, 0],
            "FIFO A routed left at full volume"
        );
    }
    #[test]
    fn output_rate_queue_bound_and_master_disable() {
        let mut apu = Apu::default();
        let mut r = registers();
        for _ in 0..32769 {
            apu.advance(512, &mut r);
        }
        assert_eq!(apu.generated, 32769);
        assert_eq!(apu.dropped, 16385);
        assert_eq!(apu.take_samples().len(), 16384);
        apu.write(0x84, 0, &mut r);
        assert_eq!(apu.mix(&r), [0, 0]);
        assert_eq!(r[0x60..0x82], [0; 34]);
    }
    #[test]
    fn square_trigger_length_envelope_and_wave_banks() {
        let mut apu = Apu::default();
        let mut r = registers();
        for (offset, value) in [(0x62, 63), (0x63, 0xf1), (0x65, 0xc0)] {
            apu.write(offset, value, &mut r);
        }
        assert_eq!(apu.read(0x84, &r) & 1, 1);
        apu.sequence(&mut r);
        assert_eq!(apu.read(0x84, &r) & 1, 0, "one length tick expires");
        apu.write(0x90, 0xab, &mut r);
        assert_eq!(apu.read(0x90, &r), 0xab);
        apu.write(0x70, 0x40, &mut r);
        assert_eq!(apu.read(0x90, &r), 0);
        apu.write(0x90, 0xcd, &mut r);
        apu.write(0x70, 0, &mut r);
        assert_eq!(apu.read(0x90, &r), 0xab);
        apu.write(0x65, 0x80, &mut r);
        apu.sequencer_step = 7;
        apu.sequence(&mut r);
        assert_eq!(apu.channels[0].volume, 14);
    }
}
