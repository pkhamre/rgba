use serde::{Deserialize, Serialize};

use crate::EmulatorError;
use crate::cartridge::Cartridge;
use crate::hardware::Hardware;
use crate::ppu::Ppu;

/// Whether a bus cycle continues a sequential transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessKind {
    /// First access or a discontinuity in the address stream.
    Nonsequential,
    /// Continuation in ascending address order.
    Sequential,
}

/// Physical access size. CPU misalignment rotation is handled by the CPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessWidth {
    /// Eight bits.
    Byte,
    /// Sixteen bits.
    Halfword,
    /// Thirty-two bits.
    Word,
}

impl AccessWidth {
    pub(crate) fn bytes(self) -> u32 {
        match self {
            Self::Byte => 1,
            Self::Halfword => 2,
            Self::Word => 4,
        }
    }
}

/// Memory map and elapsed master clocks.
///
/// Unsupported device I/O returns an error rather than acting as inert RAM.
/// WAITCNT timing excludes prefetch and video contention until those devices
/// are implemented. Open bus currently uses the last physical read latch;
/// Thumb pipeline-specific open-bus composition is not yet implemented.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bus {
    #[serde(skip)]
    bios: Box<[u8]>,
    #[serde(skip)]
    rom: Box<[u8]>,
    #[serde(deserialize_with = "crate::state::deserialize_bounded_box")]
    external_ram: Box<[u8]>,
    #[serde(deserialize_with = "crate::state::deserialize_bounded_box")]
    internal_ram: Box<[u8]>,
    #[serde(deserialize_with = "crate::state::deserialize_bounded_box")]
    palette: Box<[u8]>,
    #[serde(deserialize_with = "crate::state::deserialize_bounded_box")]
    video_ram: Box<[u8]>,
    #[serde(deserialize_with = "crate::state::deserialize_bounded_box")]
    object_ram: Box<[u8]>,
    clocks: u64,
    wait_control: u16,
    open_bus: u32,
    bios_latch: u32,
    execution_address: u32,
    pub(crate) hardware: Hardware,
    pub(crate) ppu: Ppu,
    memory_control: u32,
    pub(crate) cartridge: Cartridge,
}

impl Bus {
    /// Loads owned images without retaining caller allocations.
    ///
    /// BIOS must be exactly 16 KiB; ROM must be 192 bytes through 32 MiB.
    pub fn try_new(bios: &[u8], rom: &[u8]) -> Result<Self, EmulatorError> {
        if bios.len() != 0x4000 {
            return Err(EmulatorError::InvalidBiosSize(bios.len()));
        }
        if !(0xc0..=0x0200_0000).contains(&rom.len()) {
            return Err(EmulatorError::InvalidRomSize(rom.len()));
        }
        Ok(Self {
            cartridge: Cartridge::new(rom),
            bios: bios.into(),
            rom: rom.into(),
            external_ram: vec![0; 0x40000].into_boxed_slice(),
            internal_ram: vec![0; 0x8000].into_boxed_slice(),
            palette: vec![0; 0x400].into_boxed_slice(),
            video_ram: vec![0; 0x18000].into_boxed_slice(),
            object_ram: vec![0; 0x400].into_boxed_slice(),
            clocks: 0,
            wait_control: 0,
            open_bus: 0,
            bios_latch: 0,
            execution_address: 0,
            hardware: Hardware::default(),
            ppu: Ppu::default(),
            memory_control: 0x0d00_0020,
        })
    }

    /// Elapsed master clocks, including CPU internal cycles.
    #[must_use]
    pub fn clocks(&self) -> u64 {
        self.clocks
    }

    pub(crate) fn internal_cycles(&mut self, count: u64) {
        self.advance(count);
    }

    pub(crate) fn set_execution_address(&mut self, address: u32) {
        self.execution_address = address;
    }

    /// Reads a physical aligned bus transaction and charges its waitstates.
    pub fn read(
        &mut self,
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
    ) -> Result<u32, EmulatorError> {
        let address = address & !(width.bytes() - 1);
        self.advance(self.access_cycles(address, width, kind));
        if address >> 24 == 4 {
            return self.read_io(address, width);
        }
        if matches!(address >> 24, 0x0e | 0x0f) {
            let byte = u32::from(self.cartridge.read_backup(address));
            return Ok(match width {
                AccessWidth::Byte => byte,
                AccessWidth::Halfword => byte * 0x101,
                AccessWidth::Word => byte * 0x0101_0101,
            });
        }
        let mut value = 0;
        for index in 0..width.bytes() {
            value |= u32::from(self.read_byte(address + index)) << (index * 8);
        }
        self.open_bus = match width {
            AccessWidth::Byte => value * 0x0101_0101,
            AccessWidth::Halfword => value | (value << 16),
            AccessWidth::Word => value,
        };
        if address < 0x4000 && self.execution_address < 0x4000 {
            let aligned = address & !3;
            self.bios_latch = u32::from_le_bytes([
                self.bios[aligned as usize],
                self.bios[aligned as usize + 1],
                self.bios[aligned as usize + 2],
                self.bios[aligned as usize + 3],
            ]);
        }
        Ok(value)
    }

    /// Writes a physical aligned bus transaction and charges its waitstates.
    pub fn write(
        &mut self,
        address: u32,
        width: AccessWidth,
        value: u32,
        kind: AccessKind,
    ) -> Result<(), EmulatorError> {
        let address = address & !(width.bytes() - 1);
        self.advance(self.access_cycles(address, width, kind));
        if address >> 24 == 4 {
            return self.write_io(address, width, value);
        }
        if matches!(address >> 24, 0x0e | 0x0f) {
            self.cartridge.write_backup(address, value as u8);
            return Ok(());
        }
        if (8..=13).contains(&(address >> 24)) && width != AccessWidth::Byte {
            for offset in (0..width.bytes()).step_by(2) {
                self.cartridge
                    .write_gpio(address + offset, (value >> (offset * 8)) as u16);
            }
            return Ok(());
        }
        if width == AccessWidth::Byte {
            match address >> 24 {
                5 => {
                    let offset = (address & 0x3fe) as usize;
                    self.palette[offset] = value as u8;
                    self.palette[offset + 1] = value as u8;
                    return Ok(());
                }
                6 => {
                    let offset = Self::video_offset(address & !1);
                    // Tile-mode OBJ VRAM rejects bytes. Bitmap-mode split is
                    // deferred until display-control I/O is implemented.
                    if offset < 0x10000 {
                        self.video_ram[offset] = value as u8;
                        self.video_ram[offset + 1] = value as u8;
                    }
                    return Ok(());
                }
                7 => return Ok(()),
                _ => {}
            }
        }
        for index in 0..width.bytes() {
            self.write_byte(address + index, (value >> (index * 8)) as u8);
        }
        Ok(())
    }

    /// Computes baseline bus duration, without mutating state.
    #[must_use]
    pub fn access_cycles(&self, address: u32, width: AccessWidth, kind: AccessKind) -> u64 {
        let word = width == AccessWidth::Word;
        match address >> 24 {
            2 => {
                if word {
                    6
                } else {
                    3
                }
            }
            5 | 6 => {
                if word {
                    2
                } else {
                    1
                }
            }
            8..=13 => {
                let window = ((address >> 25) - 4) as usize;
                let first_shift = [2, 5, 8][window];
                let second_shift = [4, 7, 10][window];
                let first = [4, 3, 2, 8][((self.wait_control >> first_shift) & 3) as usize] + 1;
                let second = if self.wait_control & (1 << second_shift) != 0 {
                    2
                } else {
                    [3, 5, 9][window]
                };
                // Every 128 KiB Game Pak boundary restarts a nonsequential access.
                let nonsequential = kind == AccessKind::Nonsequential || address & 0x1ffff == 0;
                let cycles = if nonsequential { first } else { second };
                cycles + if word { second } else { 0 }
            }
            14 | 15 => [4, 3, 2, 8][(self.wait_control & 3) as usize] + 1,
            _ => 1,
        }
    }

    fn video_offset(address: u32) -> usize {
        let offset = (address & 0x1ffff) as usize;
        if offset >= 0x18000 {
            offset - 0x8000
        } else {
            offset
        }
    }

    fn read_byte(&self, address: u32) -> u8 {
        match address >> 24 {
            0 if address < 0x4000 => {
                if self.execution_address < 0x4000 {
                    self.bios[address as usize]
                } else {
                    (self.bios_latch >> ((address & 3) * 8)) as u8
                }
            }
            2 => self.external_ram[(address & 0x3ffff) as usize],
            3 => self.internal_ram[(address & 0x7fff) as usize],
            5 => self.palette[(address & 0x3ff) as usize],
            6 => self.video_ram[Self::video_offset(address)],
            7 => self.object_ram[(address & 0x3ff) as usize],
            8..=13 => {
                if let Some(value) = self.cartridge.read_gpio(address) {
                    return (value >> ((address & 1) * 8)) as u8;
                }
                let offset = (address & 0x01ff_ffff) as usize;
                self.rom.get(offset).copied().unwrap_or_else(|| {
                    let halfword = (address >> 1) as u16;
                    (halfword >> ((address & 1) * 8)) as u8
                })
            }
            _ => (self.open_bus >> ((address & 3) * 8)) as u8,
        }
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        match address >> 24 {
            2 => self.external_ram[(address & 0x3ffff) as usize] = value,
            3 => self.internal_ram[(address & 0x7fff) as usize] = value,
            5 => self.palette[(address & 0x3ff) as usize] = value,
            6 => self.video_ram[Self::video_offset(address)] = value,
            7 => self.object_ram[(address & 0x3ff) as usize] = value,
            _ => {} // BIOS, Game Pak ROM, and unmapped writes are ignored.
        }
    }

    pub(crate) fn advance(&mut self, mut cycles: u64) {
        while cycles != 0 {
            let step = cycles.min(self.hardware.next_event());
            self.clocks += step;
            self.cartridge.advance(step);
            self.hardware.advance_timers(step);
            self.hardware
                .apu
                .advance(step, &mut self.hardware.registers);
            self.hardware.line_clock += step as u16;
            if self.hardware.line_clock == 960 && self.hardware.line < 160 {
                self.ppu.render_line(
                    self.hardware.line as usize,
                    &self.hardware.registers,
                    &self.palette,
                    &self.video_ram,
                    &self.object_ram,
                );
            }
            self.hardware.display_edge();
            cycles -= step;
        }
    }

    pub(crate) fn service_dma(&mut self) -> Result<(), EmulatorError> {
        while let Some(channel) = (0..4).find(|index| self.hardware.dma[*index].pending) {
            self.hardware.dma[channel].pending = false;
            let base = 0xb0 + channel * 12;
            let control = self.hardware.half(base + 10);
            if control & 0x8000 == 0 {
                continue;
            }
            let fifo = (1..=2).contains(&channel) && control & 0x3000 == 0x3000;
            let width = if fifo || control & 0x400 != 0 {
                AccessWidth::Word
            } else {
                AccessWidth::Halfword
            };
            let stride = width.bytes();
            let raw_count =
                self.hardware.half(base + 8) as u32 & if channel == 3 { 0xffff } else { 0x3fff };
            let count = if fifo {
                4
            } else if raw_count == 0 {
                if channel == 3 { 0x10000 } else { 0x4000 }
            } else {
                raw_count
            };
            let mut source = self.hardware.dma[channel].source & !(stride - 1);
            let mut destination = self.hardware.dma[channel].destination & !(stride - 1);
            self.advance(2);
            for index in 0..count {
                let access = if index == 0 {
                    AccessKind::Nonsequential
                } else {
                    AccessKind::Sequential
                };
                let value = self.read(source, width, access)?;
                self.write(destination, width, value, access)?;
                source = match (control >> 7) & 3 {
                    1 => source.wrapping_sub(stride),
                    2 => source,
                    _ => source.wrapping_add(stride),
                };
                if !fifo {
                    destination = match (control >> 5) & 3 {
                        1 => destination.wrapping_sub(stride),
                        2 => destination,
                        _ => destination.wrapping_add(stride),
                    };
                }
            }
            self.hardware.dma[channel].source = source;
            self.hardware.dma[channel].destination = if control & 0x60 == 0x60 {
                self.hardware.dma[channel].initial_destination
            } else {
                destination
            };
            self.hardware.dma[channel].pending = false;
            if control & 0x4000 != 0 {
                self.hardware.interrupt(1 << (8 + channel));
            }
            if control & 0x200 == 0 || control & 0x3000 == 0 {
                self.hardware.set_half(base + 10, control & !0x8000);
            }
        }
        Ok(())
    }

    fn read_io(&self, address: u32, width: AccessWidth) -> Result<u32, EmulatorError> {
        if address == 0x0400_0410 {
            return Ok(0xff);
        }
        if (0x0400_0800..0x0400_0804).contains(&address) {
            return Ok((self.memory_control >> ((address & 3) * 8))
                & match width {
                    AccessWidth::Byte => 0xff,
                    AccessWidth::Halfword => 0xffff,
                    AccessWidth::Word => u32::MAX,
                });
        }
        let offset = (address - 0x0400_0000) as usize;
        if offset + width.bytes() as usize > 0x400 {
            return Err(EmulatorError::UnsupportedIo {
                address,
                write: false,
            });
        }
        let mut value = 0;
        for byte in 0..width.bytes() as usize {
            value |= u32::from(self.hardware.read_byte(offset + byte)) << (byte * 8);
        }
        Ok(value)
    }

    fn write_io(
        &mut self,
        address: u32,
        width: AccessWidth,
        value: u32,
    ) -> Result<(), EmulatorError> {
        if address == 0x0400_0410 {
            return Ok(());
        }
        if (0x0400_0800..0x0400_0804).contains(&address) {
            let shift = (address & 3) * 8;
            let mask = match width {
                AccessWidth::Byte => 0xff,
                AccessWidth::Halfword => 0xffff,
                AccessWidth::Word => u32::MAX,
            };
            self.memory_control =
                (self.memory_control & !(mask << shift)) | ((value & mask) << shift);
            return Ok(());
        }
        let offset = (address - 0x0400_0000) as usize;
        if offset + width.bytes() as usize > 0x400 {
            return Err(EmulatorError::UnsupportedIo {
                address,
                write: true,
            });
        }
        for byte in 0..width.bytes() as usize {
            self.hardware.write_byte(
                offset + byte,
                (value >> (byte * 8)) as u8,
                self.execution_address < 0x4000,
            );
            self.ppu.io_written(&self.hardware.registers, offset + byte);
        }
        self.wait_control = self.hardware.half(0x204) & 0x5fff;
        self.hardware.set_half(0x204, self.wait_control);
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> (Self, Vec<u8>) {
        let mut snapshot = self.clone();
        let registers = snapshot.hardware.registers.to_vec();
        snapshot.hardware.registers = [0; 0x400];
        (snapshot, registers)
    }

    pub(crate) fn restore_snapshot(
        &mut self,
        mut snapshot: Self,
        registers: &[u8],
    ) -> Result<(), crate::StateError> {
        if registers.len() != 0x400
            || snapshot.external_ram.len() != 0x40000
            || snapshot.internal_ram.len() != 0x8000
            || snapshot.palette.len() != 0x400
            || snapshot.video_ram.len() != 0x18000
            || snapshot.object_ram.len() != 0x400
            || snapshot.ppu.pixels.len() != 240 * 160
            || snapshot.cartridge.save.len() != self.cartridge.save.len()
            || !snapshot.cartridge.valid_snapshot()
            || !snapshot.hardware.valid_snapshot()
        {
            return Err(crate::StateError::InvalidData);
        }
        snapshot.hardware.registers.copy_from_slice(registers);
        snapshot.bios.clone_from(&self.bios);
        snapshot.rom.clone_from(&self.rom);
        *self = snapshot;
        Ok(())
    }

    pub(crate) fn rom_hash(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(&self.rom).into()
    }
}

impl crate::CpuBus for Bus {
    fn clocks(&self) -> u64 {
        self.clocks()
    }
    fn internal_cycles(&mut self, count: u64) {
        self.internal_cycles(count);
    }
    fn set_execution_address(&mut self, address: u32) {
        self.set_execution_address(address);
    }
    fn fetch(
        &mut self,
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
    ) -> Result<u32, EmulatorError> {
        self.read(address, width, kind)
    }
    fn read(
        &mut self,
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
    ) -> Result<u32, EmulatorError> {
        self.read(address, width, kind)
    }
    fn write(
        &mut self,
        address: u32,
        width: AccessWidth,
        value: u32,
        kind: AccessKind,
    ) -> Result<(), EmulatorError> {
        self.write(address, width, value, kind)
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessKind, AccessWidth, Bus};
    use crate::EmulatorError;

    fn bus() -> Bus {
        let mut bios = vec![0; 0x4000];
        bios[..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        Bus::try_new(&bios, &vec![0x42; 0x100]).expect("valid synthetic images")
    }

    fn read(bus: &mut Bus, address: u32, width: AccessWidth) -> u32 {
        bus.read(address, width, AccessKind::Nonsequential)
            .expect("implemented memory")
    }

    fn write(bus: &mut Bus, address: u32, width: AccessWidth, value: u32) {
        bus.write(address, width, value, AccessKind::Nonsequential)
            .expect("implemented memory");
    }

    #[test]
    fn immediate_dma_copies_words_and_requests_irq() {
        let mut bus = bus();
        write(&mut bus, 0x0300_0000, AccessWidth::Word, 0x1234_5678);
        write(&mut bus, 0x0400_00b0, AccessWidth::Word, 0x0300_0000);
        write(&mut bus, 0x0400_00b4, AccessWidth::Word, 0x0200_0000);
        write(&mut bus, 0x0400_00b8, AccessWidth::Word, 0xc400_0001);
        bus.service_dma().expect("DMA performs real bus transfers");
        assert_eq!(read(&mut bus, 0x0200_0000, AccessWidth::Word), 0x1234_5678);
        assert_eq!(bus.hardware.half(0x202), 1 << 8);
        assert_eq!(bus.hardware.half(0xba) & 0x8000, 0);
    }

    #[test]
    fn validates_images() {
        assert!(matches!(
            Bus::try_new(&[0; 3], &[0; 192]),
            Err(EmulatorError::InvalidBiosSize(3))
        ));
        assert!(matches!(
            Bus::try_new(&[0; 0x4000], &[]),
            Err(EmulatorError::InvalidRomSize(0))
        ));
    }

    #[test]
    fn ram_is_little_endian_aligned_and_mirrored() {
        let mut bus = bus();
        for (address, mirror) in [(0x0200_0000, 0x0204_0000), (0x0300_0000, 0x0300_8000)] {
            write(&mut bus, address + 3, AccessWidth::Word, 0x1234_5678);
            assert_eq!(read(&mut bus, mirror, AccessWidth::Word), 0x1234_5678);
            assert_eq!(read(&mut bus, mirror + 1, AccessWidth::Byte), 0x56);
            assert_eq!(read(&mut bus, mirror + 3, AccessWidth::Halfword), 0x1234);
        }
    }

    #[test]
    fn vram_upper_mirror_is_not_modulo_96_kib() {
        let mut bus = bus();
        write(&mut bus, 0x0601_0000, AccessWidth::Word, 0x1234);
        assert_eq!(read(&mut bus, 0x0601_8000, AccessWidth::Word), 0x1234);
        assert_eq!(read(&mut bus, 0x0603_8000, AccessWidth::Word), 0x1234);
        assert_eq!(read(&mut bus, 0x0600_0000, AccessWidth::Word), 0);
    }

    #[test]
    fn video_byte_write_rules() {
        let mut bus = bus();
        for address in [0x0500_0001, 0x0600_0001] {
            write(&mut bus, address, AccessWidth::Byte, 0xab);
            assert_eq!(read(&mut bus, address & !1, AccessWidth::Halfword), 0xabab);
        }
        for address in [0x0601_0000, 0x0700_0000] {
            write(&mut bus, address, AccessWidth::Byte, 0xab);
            assert_eq!(read(&mut bus, address, AccessWidth::Halfword), 0);
        }
    }

    #[test]
    fn bios_reads_are_protected_outside_bios() {
        let mut bus = bus();
        assert_eq!(read(&mut bus, 0, AccessWidth::Word), 0x1234_5678);
        bus.set_execution_address(0x0800_0000);
        assert_eq!(read(&mut bus, 0x20, AccessWidth::Word), 0x1234_5678);
        write(&mut bus, 0, AccessWidth::Word, 0);
        bus.set_execution_address(0);
        assert_eq!(read(&mut bus, 0, AccessWidth::Word), 0x1234_5678);
    }

    #[test]
    fn rom_windows_and_out_of_range_pattern() {
        let mut bus = bus();
        for address in [0x0800_0000, 0x0a00_0000, 0x0c00_0000] {
            assert_eq!(read(&mut bus, address, AccessWidth::Word), 0x4242_4242);
            write(&mut bus, address, AccessWidth::Word, 0);
            assert_eq!(read(&mut bus, address, AccessWidth::Word), 0x4242_4242);
        }
        assert_eq!(read(&mut bus, 0x0800_0100, AccessWidth::Word), 0x0081_0080);
    }

    #[test]
    fn unmapped_read_uses_last_read_latch() {
        let mut bus = bus();
        read(&mut bus, 0, AccessWidth::Word);
        assert_eq!(read(&mut bus, 0x1000_0000, AccessWidth::Word), 0x1234_5678);
    }

    #[test]
    fn baseline_access_timings_and_waitcnt() {
        let mut bus = bus();
        let n = AccessKind::Nonsequential;
        let s = AccessKind::Sequential;
        let h = AccessWidth::Halfword;
        let w = AccessWidth::Word;
        for (address, width, kind, cycles) in [
            (0x0200_0000, w, n, 6),
            (0x0200_0000, h, n, 3),
            (0x0300_0000, w, n, 1),
            (0x0500_0000, w, n, 2),
            (0x0800_0000, w, n, 8),
            (0x0800_0004, w, s, 6),
            (0x0a00_0000, w, n, 10),
            (0x0c00_0000, w, n, 14),
            (0x0802_0000, h, s, 5),
        ] {
            assert_eq!(
                bus.access_cycles(address, width, kind),
                cycles,
                "{address:08x}"
            );
        }
        write(&mut bus, 0x0400_0204, h, (2 << 2) | (1 << 4));
        assert_eq!(bus.access_cycles(0x0800_0004, w, n), 5);
        assert_eq!(bus.access_cycles(0x0800_0004, w, s), 4);
    }

    #[test]
    fn unimplemented_devices_fail_explicitly() {
        let mut bus = bus();
        assert!(matches!(
            bus.read(
                0x0400_0404,
                AccessWidth::Halfword,
                AccessKind::Nonsequential
            ),
            Err(EmulatorError::UnsupportedIo { write: false, .. })
        ));
        write(&mut bus, 0x0e00_5555, AccessWidth::Byte, 0xaa);
        assert_eq!(read(&mut bus, 0x0e00_5555, AccessWidth::Byte), 0xaa);
    }
}
