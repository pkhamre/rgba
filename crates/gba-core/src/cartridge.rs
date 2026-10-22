//! Cartridge backup and GPIO RTC. Flash completion timing is instantaneous for now.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FlashPhase {
    #[default]
    Ready,
    Unlock,
    Command,
    Program,
    Bank,
    EraseUnlock,
    EraseCommand,
    Erase,
}

#[derive(Clone, Debug)]
pub(crate) struct Cartridge {
    pub(crate) save: Vec<u8>,
    pub(crate) dirty: bool,
    flash: bool,
    bank: usize,
    identification: bool,
    phase: FlashPhase,
    rtc_present: bool,
    gpio: u16,
    direction: u16,
    enabled: bool,
    command: u8,
    bits: usize,
    data: [u8; 7],
    data_bits: usize,
    reading: bool,
    output: u16,
    control: u8,
    seconds: u64,
    cycles: u64,
}

impl Cartridge {
    pub(crate) fn new(rom: &[u8]) -> Self {
        let has = |signature: &[u8]| rom.windows(signature.len()).any(|bytes| bytes == signature);
        let large = has(b"FLASH1M_V");
        let flash = large || has(b"FLASH_V") || has(b"FLASH512_V");
        Self {
            save: vec![
                0xff;
                if large {
                    0x20000
                } else if flash {
                    0x10000
                } else {
                    0x8000
                }
            ],
            dirty: false,
            flash,
            bank: 0,
            identification: false,
            phase: FlashPhase::Ready,
            rtc_present: has(b"SIIRTC_V"),
            gpio: 0,
            direction: 0,
            enabled: false,
            command: 0,
            bits: 0,
            data: [0; 7],
            data_bits: 0,
            reading: false,
            output: 0,
            control: 0x40,
            seconds: 946684800,
            cycles: 0,
        }
    }

    pub(crate) fn advance(&mut self, cycles: u64) {
        self.cycles += cycles;
        self.seconds += self.cycles / 16_777_216;
        self.cycles %= 16_777_216;
    }

    pub(crate) fn set_timestamp(&mut self, seconds: u64) {
        self.seconds = seconds;
        self.cycles = 0;
    }

    pub(crate) fn read_backup(&self, address: u32) -> u8 {
        let offset = address as usize & 0xffff;
        if self.identification {
            return match offset {
                0 => 0xc2,
                1 => {
                    if self.save.len() == 0x20000 {
                        9
                    } else {
                        0x1c
                    }
                }
                _ => 0xff,
            };
        }
        self.save[(self.bank * 0x10000 + offset) % self.save.len()]
    }

    pub(crate) fn write_backup(&mut self, address: u32, value: u8) {
        let offset = address as usize & 0xffff;
        if !self.flash {
            let index = offset % self.save.len();
            if self.save[index] != value {
                self.save[index] = value;
                self.dirty = true;
            }
            return;
        }
        if self.phase == FlashPhase::Program {
            let index = self.bank * 0x10000 + offset;
            self.save[index] &= value;
            self.dirty = true;
            self.phase = FlashPhase::Ready;
            return;
        }
        if self.phase == FlashPhase::Bank {
            if offset == 0 && self.save.len() > 0x10000 {
                self.bank = usize::from(value & 1);
            }
            self.phase = FlashPhase::Ready;
            return;
        }
        if value == 0xf0 {
            self.identification = false;
            self.phase = FlashPhase::Ready;
            return;
        }
        self.phase = match self.phase {
            FlashPhase::Ready if offset == 0x5555 && value == 0xaa => FlashPhase::Unlock,
            FlashPhase::Unlock if offset == 0x2aaa && value == 0x55 => FlashPhase::Command,
            FlashPhase::Command if offset == 0x5555 => match value {
                0x90 => {
                    self.identification = true;
                    FlashPhase::Ready
                }
                0xa0 => FlashPhase::Program,
                0xb0 => FlashPhase::Bank,
                0x80 => FlashPhase::EraseUnlock,
                _ => FlashPhase::Ready,
            },
            FlashPhase::EraseUnlock if offset == 0x5555 && value == 0xaa => {
                FlashPhase::EraseCommand
            }
            FlashPhase::EraseCommand if offset == 0x2aaa && value == 0x55 => FlashPhase::Erase,
            FlashPhase::Erase => {
                if offset == 0x5555 && value == 0x10 {
                    self.save.fill(0xff);
                    self.dirty = true;
                } else if value == 0x30 {
                    let start = self.bank * 0x10000 + (offset & !0xfff);
                    self.save[start..start + 0x1000].fill(0xff);
                    self.dirty = true;
                }
                FlashPhase::Ready
            }
            _ => FlashPhase::Ready,
        };
    }

    pub(crate) fn read_gpio(&self, address: u32) -> Option<u16> {
        if !self.rtc_present || !self.enabled {
            return None;
        }
        match address & 0x01ff_fffe {
            0xc4 => Some((self.gpio & self.direction) | (self.output & !self.direction)),
            0xc6 => Some(self.direction),
            0xc8 => Some(u16::from(self.enabled)),
            _ => None,
        }
    }

    pub(crate) fn write_gpio(&mut self, address: u32, value: u16) {
        if !self.rtc_present {
            return;
        }
        match address & 0x01ff_fffe {
            0xc6 => {
                self.direction = value & 15;
                return;
            }
            0xc8 => {
                self.enabled = value & 1 != 0;
                return;
            }
            0xc4 => {}
            _ => return,
        }
        let old = self.gpio;
        self.gpio = value & 15;
        if value & 4 == 0 {
            self.bits = 0;
            self.command = 0;
            self.reading = false;
            self.data_bits = 0;
            return;
        }
        if old & 4 == 0 {
            self.bits = 0;
            self.command = 0;
            self.reading = false;
            self.data_bits = 0;
        }
        if self.reading && self.direction & 2 == 0 {
            if old & 1 != 0 && value & 1 == 0 {
                let index = self.bits % self.data_bits.max(1);
                self.output = u16::from((self.data[index / 8] >> (index & 7)) & 1) << 1;
                self.bits += 1;
            }
        } else if old & 1 == 0 && value & 1 != 0 && self.direction & 2 != 0 {
            if self.data_bits == 0 {
                self.command |= (((value >> 1) & 1) as u8) << self.bits;
                self.bits += 1;
                if self.bits == 8 {
                    if self.command & 0xf0 != 0x60 {
                        self.command = self.command.reverse_bits();
                    }
                    self.bits = 0;
                    self.reading = self.command & 1 != 0;
                    self.data = [0; 7];
                    self.data_bits = match self.command & 0x0e {
                        2 => 8,
                        4 => 56,
                        6 => 24,
                        _ => 0,
                    };
                    if self.command & 0x0e == 0 {
                        self.control = 0;
                    }
                    if self.reading {
                        if self.command & 0x0e == 2 {
                            self.data[0] = self.control;
                        } else {
                            let date = self.date();
                            if self.command & 0x0e == 6 {
                                self.data[..3].copy_from_slice(&date[4..]);
                            } else {
                                self.data = date;
                            }
                        }
                    }
                }
            } else if !self.reading && self.bits < self.data_bits {
                self.data[self.bits / 8] |= (((value >> 1) & 1) as u8) << (self.bits & 7);
                self.bits += 1;
                if self.bits == self.data_bits && self.command & 0x0e == 2 {
                    self.control = self.data[0] & 0x6a;
                }
            }
        }
    }

    fn date(&self) -> [u8; 7] {
        let mut days = self.seconds / 86400;
        let weekday = (days + 4) % 7;
        let mut year = 1970;
        loop {
            let length = if year % 4 == 0 { 366 } else { 365 };
            if days < length {
                break;
            }
            days -= length;
            year += 1;
        }
        let mut month = 0;
        let months = [
            31,
            if year % 4 == 0 { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        while days >= months[month] {
            days -= months[month];
            month += 1;
        }
        let bcd = |value: u64| ((value / 10) * 16 + value % 10) as u8;
        let hour = self.seconds / 3600 % 24;
        let hour = if self.control & 0x40 != 0 {
            bcd(hour)
        } else {
            bcd(hour % 12) | if hour >= 12 { 0x80 } else { 0 }
        };
        [
            bcd(year % 100),
            bcd(month as u64 + 1),
            bcd(days + 1),
            bcd(weekday),
            hour,
            bcd(self.seconds / 60 % 60),
            bcd(self.seconds % 60),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::Cartridge;

    fn command(cart: &mut Cartridge, command: u8) {
        cart.write_backup(0x5555, 0xaa);
        cart.write_backup(0x2aaa, 0x55);
        cart.write_backup(0x5555, command);
    }

    #[test]
    fn rtc_control_can_be_read_over_gpio_serial_pins() {
        let mut cart = Cartridge::new(b"SIIRTC_V001");
        cart.write_gpio(0x0800_00c8, 1);
        cart.write_gpio(0x0800_00c6, 7);
        cart.write_gpio(0x0800_00c4, 1);
        cart.write_gpio(0x0800_00c4, 5);
        for bit in 0..8 {
            let value = 4 | (((0x63 >> bit) & 1) << 1);
            cart.write_gpio(0x0800_00c4, value);
            cart.write_gpio(0x0800_00c4, value | 1);
        }
        cart.write_gpio(0x0800_00c6, 5);
        let mut control = 0;
        for bit in 0..8 {
            cart.write_gpio(0x0800_00c4, 4);
            cart.write_gpio(0x0800_00c4, 5);
            control |= ((cart.read_gpio(0x0800_00c4).expect("GPIO read enabled") >> 1) & 1) << bit;
        }
        assert_eq!(control, 0x40, "RTC reports 24-hour mode");
        assert_eq!(cart.date(), [0, 1, 1, 6, 0, 0, 0]);
        cart.advance(16_777_216);
        assert_eq!(cart.date()[6], 1);
    }

    #[test]
    fn flash_id_banking_programming_and_sector_erase() {
        let mut cart = Cartridge::new(b"FLASH1M_V103");
        command(&mut cart, 0x90);
        assert_eq!((cart.read_backup(0), cart.read_backup(1)), (0xc2, 9));
        command(&mut cart, 0xf0);
        command(&mut cart, 0xb0);
        cart.write_backup(0, 1);
        command(&mut cart, 0xa0);
        cart.write_backup(0x1234, 0x42);
        assert_eq!(cart.read_backup(0x1234), 0x42);
        command(&mut cart, 0xb0);
        cart.write_backup(0, 0);
        assert_eq!(cart.read_backup(0x1234), 0xff);
        command(&mut cart, 0xb0);
        cart.write_backup(0, 1);
        command(&mut cart, 0x80);
        cart.write_backup(0x5555, 0xaa);
        cart.write_backup(0x2aaa, 0x55);
        cart.write_backup(0x1000, 0x30);
        assert_eq!(cart.read_backup(0x1234), 0xff);
        assert!(cart.dirty);
    }
}
