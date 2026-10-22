//! Display timing, interrupts, timers and DMA register state.

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Timer {
    counter: u16,
    reload: u16,
    control: u16,
    phase: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Dma {
    pub(crate) source: u32,
    pub(crate) destination: u32,
    pub(crate) initial_destination: u32,
    pub(crate) pending: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Hardware {
    pub(crate) registers: [u8; 0x400],
    pub(crate) timers: [Timer; 4],
    pub(crate) dma: [Dma; 4],
    pub(crate) halted: bool,
    pub(crate) line: u16,
    pub(crate) line_clock: u16,
    pub(crate) frames: u64,
    pub(crate) apu: crate::apu::Apu,
}

impl Default for Hardware {
    fn default() -> Self {
        let mut hardware = Self {
            registers: [0; 0x400],
            timers: [Timer::default(); 4],
            dma: [Dma::default(); 4],
            halted: false,
            line: 0,
            line_clock: 0,
            frames: 0,
            apu: crate::apu::Apu::default(),
        };
        hardware.set_half(0, 0x0080);
        hardware.set_half(0x88, 0x0200);
        hardware.set_half(0x130, 0x03ff);
        hardware
    }
}

impl Hardware {
    pub(crate) fn half(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.registers[offset], self.registers[offset + 1]])
    }

    pub(crate) fn word(&self, offset: usize) -> u32 {
        u32::from(self.half(offset)) | (u32::from(self.half(offset + 2)) << 16)
    }

    pub(crate) fn set_half(&mut self, offset: usize, value: u16) {
        self.registers[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn interrupt(&mut self, mask: u16) {
        self.set_half(0x202, self.half(0x202) | mask);
    }

    pub(crate) fn wake_pending(&self) -> bool {
        self.half(0x200) & self.half(0x202) != 0
    }

    pub(crate) fn irq_pending(&self) -> bool {
        self.half(0x208) & 1 != 0 && self.wake_pending()
    }

    pub(crate) fn set_keys(&mut self, pressed: u16) {
        let input = !pressed & 0x03ff;
        self.set_half(0x130, input);
        let control = self.half(0x132);
        let selected = control & 0x3ff;
        let active = if control & 0x8000 != 0 {
            selected != 0 && pressed & selected == selected
        } else {
            pressed & selected != 0
        };
        if control & 0x4000 != 0 && active {
            self.interrupt(1 << 12);
        }
    }

    pub(crate) fn next_event(&self) -> u64 {
        let display = if self.line_clock < 960 {
            960 - self.line_clock
        } else if self.line_clock < 1006 {
            1006 - self.line_clock
        } else {
            1232 - self.line_clock
        };
        let mut next = u64::from(display);
        for timer in &self.timers {
            if timer.control & 0x84 != 0x80 {
                continue;
            }
            let divisor = [1, 64, 256, 1024][(timer.control & 3) as usize];
            let remaining = (0x10000 - u64::from(timer.counter)) * divisor - timer.phase;
            next = next.min(remaining.max(1));
        }
        next.min(self.apu.next_sample()).max(1)
    }

    pub(crate) fn advance_timers(&mut self, cycles: u64) {
        let mut overflow = 0;
        for index in 0..4 {
            let timer = &mut self.timers[index];
            if timer.control & 0x80 == 0 {
                overflow = 0;
                continue;
            }
            let ticks = if index != 0 && timer.control & 4 != 0 {
                overflow
            } else {
                let divisor = [1, 64, 256, 1024][(timer.control & 3) as usize];
                timer.phase += cycles;
                let ticks = timer.phase / divisor;
                timer.phase %= divisor;
                ticks
            };
            let until_overflow = 0x10000 - u64::from(timer.counter);
            overflow = if ticks < until_overflow {
                timer.counter = timer.counter.wrapping_add(ticks as u16);
                0
            } else {
                let period = 0x10000 - u64::from(timer.reload);
                let remaining = ticks - until_overflow;
                timer.counter = timer.reload.wrapping_add((remaining % period) as u16);
                1 + remaining / period
            };
            let irq = timer.control & 0x40 != 0 && overflow != 0;
            if irq {
                self.interrupt(1 << (3 + index));
            }
            if index < 2 && overflow != 0 {
                for fifo in 0..2 {
                    let selector = if fifo == 0 { 10 } else { 14 };
                    if usize::from((self.half(0x82) >> selector) & 1) == index {
                        self.apu.clock_fifo(fifo, overflow);
                        if self.apu.fifo_len(fifo) <= 16 {
                            for channel in 1..=2 {
                                let control = self.half(0xba + channel * 12);
                                if control & 0xb000 == 0xb000
                                    && self.dma[channel].destination
                                        == 0x0400_00a0 + fifo as u32 * 4
                                {
                                    self.dma[channel].pending = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn trigger_dma(&mut self, timing: u16) {
        for channel in 0..4 {
            let control = self.half(0xba + channel * 12);
            if control & 0x8000 != 0 && (control >> 12) & 3 == timing {
                self.dma[channel].pending = true;
            }
        }
    }

    pub(crate) fn display_edge(&mut self) {
        if self.line_clock == 1006 {
            self.set_half(4, self.half(4) | 2);
            if self.half(4) & 0x10 != 0 {
                self.interrupt(2);
            }
            if self.line < 160 {
                self.trigger_dma(2);
            }
        }
        if self.line_clock == 1232 {
            self.line_clock = 0;
            self.line += 1;
            if self.line == 228 {
                self.line = 0;
                self.frames += 1;
            }
            let mut status = self.half(4) & !7;
            if (160..227).contains(&self.line) {
                status |= 1;
            }
            if self.line == self.half(4) >> 8 {
                status |= 4;
            }
            self.set_half(4, status);
            if status & 4 != 0 && status & 0x20 != 0 {
                self.interrupt(4);
            }
            if self.line == 160 {
                if status & 8 != 0 {
                    self.interrupt(1);
                }
                self.trigger_dma(1);
            }
        }
    }

    pub(crate) fn read_byte(&self, offset: usize) -> u8 {
        if (0x60..0xa8).contains(&offset) {
            return self.apu.read(offset, &self.registers);
        }
        if (0x100..0x110).contains(&offset) && offset & 3 < 2 {
            return (self.timers[(offset - 0x100) / 4].counter >> ((offset & 1) * 8)) as u8;
        }
        if offset == 6 {
            return self.line as u8;
        }
        if offset == 7 {
            return 0;
        }
        if (0xb0..0xe0).contains(&offset) && (offset - 0xb0) % 12 < 10 {
            return 0;
        }
        self.registers[offset]
    }

    pub(crate) fn write_byte(&mut self, offset: usize, value: u8, bios: bool) {
        if (0x60..0xa8).contains(&offset) {
            self.apu.write(offset, value, &mut self.registers);
            return;
        }
        match offset {
            6 | 7 | 0x130 | 0x131 => return,
            0x202 | 0x203 => {
                self.registers[offset] &= !value;
                return;
            }
            4 => {
                self.registers[4] = (self.registers[4] & 7) | (value & 0x38);
                return;
            }
            0x208 => {
                self.registers[offset] = value & 1;
                return;
            }
            0x300 => {
                if bios {
                    self.registers[offset] |= value & 1;
                }
                return;
            }
            0x301 => {
                self.halted = true;
                return;
            }
            _ => {}
        }
        if (0x100..0x110).contains(&offset) {
            let index = (offset - 0x100) / 4;
            let field = offset & 3;
            if field < 2 {
                let shift = field * 8;
                self.timers[index].reload =
                    (self.timers[index].reload & !(0xff << shift)) | (u16::from(value) << shift);
                if self.timers[index].control & 0x80 == 0 {
                    self.timers[index].counter = self.timers[index].reload;
                }
            } else if field == 2 {
                let timer = &mut self.timers[index];
                if timer.control & 0x80 == 0 && value & 0x80 != 0 {
                    timer.counter = timer.reload;
                    timer.phase = 0;
                }
                timer.control = u16::from(value & 0xc7);
            }
        }
        let old = self.registers[offset];
        self.registers[offset] = value;
        if (0xbb..0xe0).contains(&offset)
            && (offset - 0xbb).is_multiple_of(12)
            && old & 0x80 == 0
            && value & 0x80 != 0
        {
            let channel = (offset - 0xbb) / 12;
            let base = 0xb0 + channel * 12;
            let source = self.word(base)
                & if channel == 0 {
                    0x07ff_ffff
                } else {
                    0x0fff_ffff
                };
            let destination = self.word(base + 4)
                & if channel == 3 {
                    0x0fff_ffff
                } else {
                    0x07ff_ffff
                };
            self.dma[channel] = Dma {
                source,
                destination,
                initial_destination: destination,
                pending: value & 0x30 == 0,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Hardware;

    #[test]
    fn selected_timer_consumes_real_fifo_bytes_and_requests_dma() {
        let mut hw = Hardware::default();
        hw.write_byte(0x84, 0x80, true);
        for value in 0..4 {
            hw.write_byte(0xa0, value, true);
        }
        hw.dma[1].destination = 0x0400_00a0;
        hw.set_half(0xc6, 0xb000);
        hw.write_byte(0x100, 0xff, true);
        hw.write_byte(0x101, 0xff, true);
        hw.write_byte(0x102, 0x80, true);
        hw.advance_timers(1);
        assert_eq!(hw.apu.fifo_len(0), 3);
        assert!(hw.dma[1].pending);
        hw.write_byte(0x83, 8, true);
        assert_eq!(hw.apu.fifo_len(0), 0);
        assert_eq!(hw.read_byte(0x83) & 0x88, 0, "reset bits are strobes");
    }

    #[test]
    fn timer_reload_cascade_and_irq() {
        let mut hw = Hardware::default();
        hw.write_byte(0x100, 0xfe, true);
        hw.write_byte(0x101, 0xff, true);
        hw.write_byte(0x102, 0xc0, true);
        hw.write_byte(0x104, 0xff, true);
        hw.write_byte(0x105, 0xff, true);
        hw.write_byte(0x106, 0xc4, true);
        hw.advance_timers(5);
        assert_eq!(hw.timers[0].counter, 0xffff);
        assert_eq!(hw.timers[1].counter, 0xffff);
        assert_eq!(hw.half(0x202) & 0x18, 0x18);
    }

    #[test]
    fn interrupt_acknowledge_and_keypad() {
        let mut hw = Hardware::default();
        hw.set_half(0x132, 0x4001);
        hw.set_keys(1);
        assert_eq!(hw.half(0x130), 0x3fe);
        assert_eq!(hw.half(0x202), 1 << 12);
        hw.write_byte(0x203, 0x10, true);
        assert_eq!(hw.half(0x202), 0);
    }

    #[test]
    fn line_status_and_vblank_dma() {
        let mut hw = Hardware::default();
        hw.set_half(4, 8);
        hw.line = 159;
        hw.line_clock = 1232;
        hw.display_edge();
        assert_eq!(hw.line, 160);
        assert_ne!(hw.half(4) & 1, 0);
        assert_ne!(hw.half(0x202) & 1, 0);
    }
}
