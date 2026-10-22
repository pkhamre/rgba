use crate::alu;
use crate::alu::{ArithmeticResult, ShiftKind, ShiftSource};
use crate::multiply;
use crate::multiply::MultiplyKind;
use crate::{
    AccessKind, AccessWidth, CpuBus, EmulatorError, ProcessorMode, ProcessorStatus, StepTrace,
};

/// ARM7TDMI interpreter with explicit two-entry prefetch state and banked registers.
///
/// Baseline N/S/I cycles are charged to the bus. Prefetch-buffer acceleration,
/// bus arbitration, and exception entry overlap still require hardware validation.
/// An error is terminal: do not resume a partially executed instruction.
#[derive(Clone, Debug)]
pub struct Cpu {
    registers: [u32; 15],
    address: u32,
    status: ProcessorStatus,
    saved_status: [ProcessorStatus; 6],
    banks: [[u32; 2]; 6],
    normal_high: [u32; 5],
    fiq_high: [u32; 5],
    pipeline: Option<[u32; 2]>,
    next_fetch: AccessKind,
    branched: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self {
            registers: [0; 15],
            address: 0,
            status: ProcessorStatus::default(),
            saved_status: [ProcessorStatus::default(); 6],
            banks: [[0; 2]; 6],
            normal_high: [0; 5],
            fiq_high: [0; 5],
            pipeline: None,
            next_fetch: AccessKind::Sequential,
            branched: false,
        }
    }
}

impl Cpu {
    /// Current instruction address. Reset starts at the BIOS vector at zero.
    #[must_use]
    pub fn instruction_address(&self) -> u32 {
        self.address
    }

    /// Current privilege, state, and arithmetic flags.
    #[must_use]
    pub fn status(&self) -> ProcessorStatus {
        self.status
    }

    /// Architectural register values. R15 reads address + 8 (ARM) or + 4 (Thumb).
    #[must_use]
    pub fn registers(&self) -> [u32; 16] {
        let mut values = [0; 16];
        values[..15].copy_from_slice(&self.registers);
        values[15] = self.read_register(15);
        values
    }

    /// Executes one instruction, including bus/internal cycle accounting.
    pub fn step(&mut self, bus: &mut impl CpuBus) -> Result<StepTrace, EmulatorError> {
        let clock_before = bus.clocks();
        bus.set_execution_address(self.address);
        if self.pipeline.is_none() {
            self.refill(bus)?;
        }
        let [opcode, next] = self.pipeline.expect("pipeline was refilled");
        let address = self.address;
        let thumb = self.status.thumb();
        let width = if thumb {
            AccessWidth::Halfword
        } else {
            AccessWidth::Word
        };
        let fetched = bus.fetch(
            address.wrapping_add(width.bytes() * 2),
            width,
            self.next_fetch,
        )?;
        self.next_fetch = AccessKind::Sequential;
        self.branched = false;
        let condition_passed = thumb || self.status.condition_passed(opcode >> 28);
        if condition_passed {
            if thumb {
                self.execute_thumb(opcode, bus)?;
            } else {
                self.execute_arm(opcode, bus)?;
            }
        }
        if self.branched {
            self.refill(bus)?;
        } else {
            self.address = self.address.wrapping_add(width.bytes());
            self.pipeline = Some([next, fetched]);
        }
        Ok(StepTrace {
            address,
            opcode,
            thumb,
            condition_passed,
            clock_before,
            clock_after: bus.clocks(),
            registers: self.registers(),
            status: self.status.bits(),
        })
    }

    fn refill(&mut self, bus: &mut impl CpuBus) -> Result<(), EmulatorError> {
        let width = if self.status.thumb() {
            AccessWidth::Halfword
        } else {
            AccessWidth::Word
        };
        bus.set_execution_address(self.address);
        let first = bus.fetch(self.address, width, AccessKind::Nonsequential)?;
        let second = bus.fetch(
            self.address.wrapping_add(width.bytes()),
            width,
            AccessKind::Sequential,
        )?;
        self.pipeline = Some([first, second]);
        self.next_fetch = AccessKind::Sequential;
        Ok(())
    }

    fn read_register(&self, index: usize) -> u32 {
        if index == 15 {
            self.address
                .wrapping_add(if self.status.thumb() { 4 } else { 8 })
        } else {
            self.registers[index]
        }
    }

    fn write_register(&mut self, index: usize, value: u32) {
        if index == 15 {
            self.branch(value);
        } else {
            self.registers[index] = value;
        }
    }

    fn branch(&mut self, target: u32) {
        self.address = target & if self.status.thumb() { !1 } else { !3 };
        self.pipeline = None;
        self.branched = true;
    }

    fn exchange(&mut self, target: u32) {
        self.status.set_thumb(target & 1 != 0);
        self.branch(target);
    }

    fn replace_status(&mut self, status: ProcessorStatus) {
        let old_mode = self.status.mode();
        let new_mode = status.mode();
        if old_mode != new_mode {
            self.banks[old_mode.bank()] = [self.registers[13], self.registers[14]];
            let new_bank = self.banks[new_mode.bank()];
            self.registers[13] = new_bank[0];
            self.registers[14] = new_bank[1];
            if old_mode == ProcessorMode::Fiq {
                self.fiq_high.copy_from_slice(&self.registers[8..13]);
                self.registers[8..13].copy_from_slice(&self.normal_high);
            } else if new_mode == ProcessorMode::Fiq {
                self.normal_high.copy_from_slice(&self.registers[8..13]);
                self.registers[8..13].copy_from_slice(&self.fiq_high);
            }
        }
        self.status = status;
    }

    fn restore_status(&mut self) {
        let bank = self.status.mode().bank();
        if bank != 0 {
            self.replace_status(self.saved_status[bank]);
        }
    }

    fn exception(&mut self, mode: ProcessorMode, vector: u32, return_address: u32) {
        let old = self.status;
        let bits = (old.bits() & !0x3f) | mode as u32 | 0x80;
        self.replace_status(ProcessorStatus::try_new(bits).expect("exception mode is valid"));
        self.saved_status[mode.bank()] = old;
        self.registers[14] = return_address;
        self.branch(vector);
    }

    fn undefined_instruction(&mut self, bus: &mut impl CpuBus) {
        self.internal_cycles(bus, 1);
        let return_address = self
            .address
            .wrapping_add(if self.status.thumb() { 2 } else { 4 });
        self.exception(ProcessorMode::Undefined, 4, return_address);
    }

    fn internal_cycles(&mut self, bus: &mut impl CpuBus, count: u64) {
        bus.internal_cycles(count);
        // The CPU relinquishes the sequential code stream during I cycles.
        // GamePak prefetch can hide the resulting N access once implemented.
        self.next_fetch = AccessKind::Nonsequential;
    }

    fn arithmetic_flags(&mut self, result: ArithmeticResult) -> u32 {
        self.status.set_nz(result.value);
        self.status.set_carry(result.carry);
        self.status.set_overflow(result.overflow);
        result.value
    }

    fn load(
        &mut self,
        bus: &mut impl CpuBus,
        address: u32,
        width: AccessWidth,
    ) -> Result<u32, EmulatorError> {
        let value = bus.read(address, width, AccessKind::Nonsequential)?;
        // A data access breaks the instruction-fetch sequential stream.
        self.next_fetch = AccessKind::Nonsequential;
        self.internal_cycles(bus, 1);
        Ok(match width {
            AccessWidth::Word => value.rotate_right((address & 3) * 8),
            AccessWidth::Halfword => value.rotate_right((address & 1) * 8),
            AccessWidth::Byte => value,
        })
    }

    fn store(
        &mut self,
        bus: &mut impl CpuBus,
        address: u32,
        width: AccessWidth,
        value: u32,
    ) -> Result<(), EmulatorError> {
        bus.write(address, width, value, AccessKind::Nonsequential)?;
        self.next_fetch = AccessKind::Nonsequential;
        Ok(())
    }

    fn operand_two(&mut self, opcode: u32, bus: &mut impl CpuBus) -> (u32, bool, bool) {
        if opcode & (1 << 25) != 0 {
            let amount = ((opcode >> 8) & 15) * 2;
            let value = (opcode & 0xff).rotate_right(amount);
            return (
                value,
                if amount == 0 {
                    self.status.carry()
                } else {
                    value >> 31 != 0
                },
                false,
            );
        }
        let register_shift = opcode & 0x10 != 0;
        let index = (opcode & 15) as usize;
        let mut value = self.read_register(index);
        let amount = if register_shift {
            self.internal_cycles(bus, 1);
            if index == 15 {
                value = value.wrapping_add(4);
            }
            self.read_register(((opcode >> 8) & 15) as usize) & 0xff
        } else {
            (opcode >> 7) & 31
        };
        let result = alu::shift(
            value,
            ShiftKind::decode(opcode >> 5),
            amount,
            self.status.carry(),
            if register_shift {
                ShiftSource::Register
            } else {
                ShiftSource::Immediate
            },
        );
        (result.value, result.carry, register_shift)
    }

    fn execute_arm(&mut self, opcode: u32, bus: &mut impl CpuBus) -> Result<(), EmulatorError> {
        if opcode & 0x0fff_fff0 == 0x012f_ff10 {
            self.exchange(self.read_register((opcode & 15) as usize));
        } else if opcode & 0x0fbf_0fff == 0x010f_0000 {
            let bits = if opcode & (1 << 22) != 0 && self.status.mode().bank() != 0 {
                self.saved_status[self.status.mode().bank()].bits()
            } else {
                self.status.bits()
            };
            self.write_register(((opcode >> 12) & 15) as usize, bits);
        } else if opcode & 0x0fb0_fff0 == 0x0120_f000 || opcode & 0x0fb0_f000 == 0x0320_f000 {
            self.arm_status_transfer(opcode)?;
        } else if opcode & 0x0f80_00f0 == 0x0080_0090 {
            self.arm_multiply_long(opcode, bus);
        } else if opcode & 0x0fc0_00f0 == 0x0000_0090 {
            self.arm_multiply(opcode, bus);
        } else if opcode & 0x0fb0_0ff0 == 0x0100_0090 {
            self.arm_swap(opcode, bus)?;
        } else if opcode & 0x0e00_0090 == 0x0000_0090 {
            self.arm_halfword_transfer(opcode, bus)?;
        } else {
            match (opcode >> 25) & 7 {
                0 | 1 => self.arm_data_processing(opcode, bus)?,
                2 | 3 => self.arm_single_transfer(opcode, bus)?,
                4 => self.arm_block_transfer(opcode, bus)?,
                5 => {
                    let offset = (((opcode & 0x00ff_ffff) << 8) as i32 >> 6) as u32;
                    let target = self.read_register(15).wrapping_add(offset);
                    if opcode & (1 << 24) != 0 {
                        self.registers[14] = self.address.wrapping_add(4);
                    }
                    self.branch(target);
                }
                7 if opcode & (1 << 24) != 0 => {
                    self.exception(ProcessorMode::Supervisor, 8, self.address.wrapping_add(4));
                }
                _ => self.undefined_instruction(bus),
            }
        }
        Ok(())
    }

    fn arm_status_transfer(&mut self, opcode: u32) -> Result<(), EmulatorError> {
        let operand = if opcode & (1 << 25) != 0 {
            (opcode & 0xff).rotate_right(((opcode >> 8) & 15) * 2)
        } else {
            self.read_register((opcode & 15) as usize)
        };
        let saved = opcode & (1 << 22) != 0;
        let mut mask = 0;
        for field in 0..4 {
            if opcode & (1 << (16 + field)) != 0 {
                mask |= 0xff << (field * 8);
            }
        }
        // Retain explicitly written fields for MRS round trips. Software should
        // not rely on reserved PSR bits; this policy follows the reference core.
        if saved {
            let bank = self.status.mode().bank();
            if bank != 0 {
                let old = self.saved_status[bank].bits();
                self.saved_status[bank] =
                    ProcessorStatus::try_new((old & !mask) | (operand & mask))?;
            }
        } else {
            mask &= !0x20; // MSR cannot exchange instruction sets.
            if self.status.mode() == ProcessorMode::User {
                mask &= 0xff00_0000;
            }
            let bits = (self.status.bits() & !mask) | (operand & mask);
            self.replace_status(ProcessorStatus::try_new(bits)?);
        }
        Ok(())
    }

    fn arm_data_processing(
        &mut self,
        opcode: u32,
        bus: &mut impl CpuBus,
    ) -> Result<(), EmulatorError> {
        let operation = (opcode >> 21) & 15;
        let set_flags = opcode & (1 << 20) != 0;
        if (8..=11).contains(&operation) && !set_flags {
            self.undefined_instruction(bus);
            return Ok(());
        }
        let destination = ((opcode >> 12) & 15) as usize;
        let source = ((opcode >> 16) & 15) as usize;
        let (right, shifter_carry, register_shift) = self.operand_two(opcode, bus);
        let left = self
            .read_register(source)
            .wrapping_add(if source == 15 && register_shift { 4 } else { 0 });
        let carry = self.status.carry();
        let arithmetic = match operation {
            2 | 10 => Some(alu::add_with_carry(left, !right, true)),
            3 => Some(alu::add_with_carry(right, !left, true)),
            4 | 11 => Some(alu::add_with_carry(left, right, false)),
            5 => Some(alu::add_with_carry(left, right, carry)),
            6 => Some(alu::add_with_carry(left, !right, carry)),
            7 => Some(alu::add_with_carry(right, !left, carry)),
            _ => None,
        };
        let value = if let Some(result) = arithmetic {
            result.value
        } else {
            match operation {
                0 | 8 => left & right,
                1 | 9 => left ^ right,
                12 => left | right,
                13 => right,
                14 => left & !right,
                15 => !right,
                _ => unreachable!("arithmetic operations handled above"),
            }
        };
        let test = (8..=11).contains(&operation);
        if set_flags {
            if destination == 15 && (test || self.status.mode().bank() != 0) {
                // Legacy {P} test encodings restore SPSR without writing PC.
                // In User/System there is no SPSR, so this is a no-op.
                self.restore_status();
            } else if let Some(result) = arithmetic {
                self.arithmetic_flags(result);
            } else {
                self.status.set_nz(value);
                self.status.set_carry(shifter_carry);
            }
        }
        if !test {
            self.write_register(destination, value);
        }
        Ok(())
    }

    fn arm_multiply(&mut self, opcode: u32, bus: &mut impl CpuBus) {
        let left = self.read_register((opcode & 15) as usize);
        let right = self.read_register(((opcode >> 8) & 15) as usize);
        let accumulate = opcode & (1 << 21) != 0;
        let accumulator = if accumulate {
            self.read_register(((opcode >> 12) & 15) as usize)
        } else {
            0
        };
        let value = left.wrapping_mul(right).wrapping_add(accumulator);
        self.internal_cycles(
            bus,
            alu::multiply_cycles(right, true) + u64::from(accumulate),
        );
        if opcode & (1 << 20) != 0 {
            self.status.set_nz(value);
            self.status.set_carry(multiply::carry(
                left,
                right,
                u64::from(accumulator),
                MultiplyKind::Short,
            ));
        }
        self.write_register(((opcode >> 16) & 15) as usize, value);
    }

    fn arm_multiply_long(&mut self, opcode: u32, bus: &mut impl CpuBus) {
        let left = self.read_register((opcode & 15) as usize);
        let right = self.read_register(((opcode >> 8) & 15) as usize);
        let signed = opcode & (1 << 22) != 0;
        let accumulate = opcode & (1 << 21) != 0;
        let low = ((opcode >> 12) & 15) as usize;
        let high = ((opcode >> 16) & 15) as usize;
        let mut value = if signed {
            (i64::from(left as i32) * i64::from(right as i32)) as u64
        } else {
            u64::from(left) * u64::from(right)
        };
        let accumulator = if accumulate {
            (u64::from(self.read_register(high)) << 32) | u64::from(self.read_register(low))
        } else {
            0
        };
        value = value.wrapping_add(accumulator);
        self.internal_cycles(
            bus,
            alu::multiply_cycles(right, signed) + 1 + u64::from(accumulate),
        );
        if opcode & (1 << 20) != 0 {
            // N comes from bit 63; Z considers both halves. V is preserved.
            self.status
                .set_nz(((value >> 32) as u32 & 0x8000_0000) | u32::from(value != 0));
            let kind = if signed {
                MultiplyKind::LongSigned
            } else {
                MultiplyKind::LongUnsigned
            };
            self.status
                .set_carry(multiply::carry(left, right, accumulator, kind));
        }
        self.write_register(low, value as u32);
        self.write_register(high, (value >> 32) as u32);
    }

    fn arm_swap(&mut self, opcode: u32, bus: &mut impl CpuBus) -> Result<(), EmulatorError> {
        let address = self.read_register(((opcode >> 16) & 15) as usize);
        let source = self.read_register((opcode & 15) as usize);
        let width = if opcode & (1 << 22) != 0 {
            AccessWidth::Byte
        } else {
            AccessWidth::Word
        };
        let value = self.load(bus, address, width)?;
        self.store(bus, address, width, source)?;
        self.write_register(((opcode >> 12) & 15) as usize, value);
        Ok(())
    }

    fn arm_single_transfer(
        &mut self,
        opcode: u32,
        bus: &mut impl CpuBus,
    ) -> Result<(), EmulatorError> {
        let base_index = ((opcode >> 16) & 15) as usize;
        let destination = ((opcode >> 12) & 15) as usize;
        let base = self.read_register(base_index);
        let offset = if opcode & (1 << 25) != 0 {
            if opcode & 0x10 != 0 {
                self.undefined_instruction(bus);
                return Ok(());
            }
            alu::shift(
                self.read_register((opcode & 15) as usize),
                ShiftKind::decode(opcode >> 5),
                (opcode >> 7) & 31,
                self.status.carry(),
                ShiftSource::Immediate,
            )
            .value
        } else {
            opcode & 0xfff
        };
        let adjusted = if opcode & (1 << 23) != 0 {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let pre = opcode & (1 << 24) != 0;
        let address = if pre { adjusted } else { base };
        let load = opcode & (1 << 20) != 0;
        let writeback = !pre || opcode & (1 << 21) != 0;
        let width = if opcode & (1 << 22) != 0 {
            AccessWidth::Byte
        } else {
            AccessWidth::Word
        };
        if load {
            let value = self.load(bus, address, width)?;
            if writeback && base_index != destination {
                self.write_register(base_index, adjusted);
            }
            self.write_register(destination, value);
        } else {
            let value = self
                .read_register(destination)
                .wrapping_add(if destination == 15 { 4 } else { 0 });
            self.store(bus, address, width, value)?;
            if writeback {
                self.write_register(base_index, adjusted);
            }
        }
        Ok(())
    }

    fn signed_load(
        &mut self,
        bus: &mut impl CpuBus,
        address: u32,
        operation: u32,
    ) -> Result<u32, EmulatorError> {
        match operation {
            1 => self.load(bus, address, AccessWidth::Halfword),
            2 => Ok(self.load(bus, address, AccessWidth::Byte)? as u8 as i8 as i32 as u32),
            3 if address & 1 != 0 => {
                // The physical access is still a halfword. Rotation places its
                // upper byte at bit zero before the ARM7 sign extension.
                Ok(self.load(bus, address, AccessWidth::Halfword)? as u8 as i8 as i32 as u32)
            }
            3 => Ok(self.load(bus, address, AccessWidth::Halfword)? as u16 as i16 as i32 as u32),
            _ => unreachable!("halfword load operation validated by decoder"),
        }
    }

    fn arm_halfword_transfer(
        &mut self,
        opcode: u32,
        bus: &mut impl CpuBus,
    ) -> Result<(), EmulatorError> {
        let operation = (opcode >> 5) & 3;
        let load = opcode & (1 << 20) != 0;
        if operation == 0 || (!load && operation != 1) {
            self.undefined_instruction(bus);
            return Ok(());
        }
        let base_index = ((opcode >> 16) & 15) as usize;
        let destination = ((opcode >> 12) & 15) as usize;
        let base = self.read_register(base_index);
        let offset = if opcode & (1 << 22) != 0 {
            ((opcode >> 4) & 0xf0) | (opcode & 15)
        } else {
            self.read_register((opcode & 15) as usize)
        };
        let adjusted = if opcode & (1 << 23) != 0 {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let pre = opcode & (1 << 24) != 0;
        let address = if pre { adjusted } else { base };
        let writeback = !pre || opcode & (1 << 21) != 0;
        if load {
            let value = self.signed_load(bus, address, operation)?;
            if writeback && base_index != destination {
                self.write_register(base_index, adjusted);
            }
            self.write_register(destination, value);
        } else {
            let value = self
                .read_register(destination)
                .wrapping_add(if destination == 15 { 4 } else { 0 });
            self.store(bus, address, AccessWidth::Halfword, value)?;
            if writeback {
                self.write_register(base_index, adjusted);
            }
        }
        Ok(())
    }

    fn user_register(&self, index: usize) -> u32 {
        match index {
            8..=12 if self.status.mode() == ProcessorMode::Fiq => self.normal_high[index - 8],
            13 | 14 if self.status.mode().bank() != 0 => self.banks[0][index - 13],
            _ => self.read_register(index),
        }
    }

    fn set_user_register(&mut self, index: usize, value: u32) {
        match index {
            8..=12 if self.status.mode() == ProcessorMode::Fiq => {
                self.normal_high[index - 8] = value
            }
            13 | 14 if self.status.mode().bank() != 0 => self.banks[0][index - 13] = value,
            _ => self.write_register(index, value),
        }
    }

    fn arm_block_transfer(
        &mut self,
        opcode: u32,
        bus: &mut impl CpuBus,
    ) -> Result<(), EmulatorError> {
        let base_index = ((opcode >> 16) & 15) as usize;
        let base = self.read_register(base_index);
        let original_list = opcode & 0xffff;
        let list = if original_list == 0 {
            1 << 15
        } else {
            original_list
        };
        let count = if original_list == 0 {
            16
        } else {
            list.count_ones()
        };
        let up = opcode & (1 << 23) != 0;
        let pre = opcode & (1 << 24) != 0;
        let load = opcode & (1 << 20) != 0;
        let writeback = opcode & (1 << 21) != 0;
        let final_base = if up {
            base.wrapping_add(count * 4)
        } else {
            base.wrapping_sub(count * 4)
        };
        let mut address = if up {
            base.wrapping_add(if pre { 4 } else { 0 })
        } else {
            final_base.wrapping_add(if pre { 0 } else { 4 })
        };
        let restore = load && opcode & (1 << 22) != 0 && list & (1 << 15) != 0;
        let user = opcode & (1 << 22) != 0 && !restore;
        let first = list.trailing_zeros() as usize;
        let mut loaded_pc = None;
        for index in 0..16 {
            if list & (1 << index) == 0 {
                continue;
            }
            let kind = if index == first {
                AccessKind::Nonsequential
            } else {
                AccessKind::Sequential
            };
            if load {
                let value = bus.read(address, AccessWidth::Word, kind)?;
                if index == 15 {
                    loaded_pc = Some(value);
                } else if user {
                    self.set_user_register(index, value);
                } else {
                    self.write_register(index, value);
                }
            } else {
                let value = if writeback && index == base_index && index != first {
                    final_base
                } else if index == 15 {
                    self.read_register(15)
                        .wrapping_add(if self.status.thumb() { 2 } else { 4 })
                } else if user {
                    self.user_register(index)
                } else {
                    self.read_register(index)
                };
                bus.write(address, AccessWidth::Word, value, kind)?;
            }
            address = address.wrapping_add(4);
        }
        if writeback && !(load && list & (1 << base_index) != 0) {
            self.write_register(base_index, final_base);
        }
        if load {
            self.internal_cycles(bus, 1);
        }
        self.next_fetch = AccessKind::Nonsequential;
        if restore {
            self.restore_status();
        }
        if let Some(target) = loaded_pc {
            self.branch(target);
        }
        Ok(())
    }

    fn execute_thumb(&mut self, opcode: u32, bus: &mut impl CpuBus) -> Result<(), EmulatorError> {
        match opcode >> 13 {
            0 => self.thumb_shift_add(opcode, bus),
            1 => self.thumb_immediate(opcode),
            _ => return self.thumb_other(opcode, bus),
        }
        Ok(())
    }

    fn thumb_shift_add(&mut self, opcode: u32, _bus: &mut impl CpuBus) {
        let destination = (opcode & 7) as usize;
        let left = self.read_register(((opcode >> 3) & 7) as usize);
        if opcode & 0x1800 == 0x1800 {
            let right = if opcode & 0x400 != 0 {
                (opcode >> 6) & 7
            } else {
                self.read_register(((opcode >> 6) & 7) as usize)
            };
            let subtract = opcode & 0x200 != 0;
            let result = if subtract {
                alu::add_with_carry(left, !right, true)
            } else {
                alu::add_with_carry(left, right, false)
            };
            let value = self.arithmetic_flags(result);
            self.write_register(destination, value);
        } else {
            let result = alu::shift(
                left,
                ShiftKind::decode(opcode >> 11),
                (opcode >> 6) & 31,
                self.status.carry(),
                ShiftSource::Immediate,
            );
            self.status.set_nz(result.value);
            self.status.set_carry(result.carry);
            self.write_register(destination, result.value);
        }
    }

    fn thumb_immediate(&mut self, opcode: u32) {
        let index = ((opcode >> 8) & 7) as usize;
        let left = self.read_register(index);
        let right = opcode & 0xff;
        let operation = (opcode >> 11) & 3;
        let value = match operation {
            0 => {
                self.status.set_nz(right);
                right
            }
            1 | 3 => self.arithmetic_flags(alu::add_with_carry(left, !right, true)),
            2 => self.arithmetic_flags(alu::add_with_carry(left, right, false)),
            _ => unreachable!("two-bit immediate operation"),
        };
        if operation != 1 {
            self.write_register(index, value);
        }
    }

    fn thumb_other(&mut self, opcode: u32, bus: &mut impl CpuBus) -> Result<(), EmulatorError> {
        match opcode & 0xfc00 {
            0x4000 => {
                self.thumb_alu(opcode, bus);
                return Ok(());
            }
            0x4400 => {
                self.thumb_high(opcode, bus);
                return Ok(());
            }
            _ => {}
        }
        if opcode & 0xf800 == 0x4800 {
            let address = (self.read_register(15) & !3).wrapping_add((opcode & 0xff) * 4);
            let value = self.load(bus, address, AccessWidth::Word)?;
            self.write_register(((opcode >> 8) & 7) as usize, value);
        } else if opcode & 0xf000 == 0x5000 {
            self.thumb_register_transfer(opcode, bus)?;
        } else if opcode & 0xe000 == 0x6000 || opcode & 0xf000 == 0x8000 {
            self.thumb_immediate_transfer(opcode, bus)?;
        } else if opcode & 0xf000 == 0x9000 {
            let index = ((opcode >> 8) & 7) as usize;
            let address = self.registers[13].wrapping_add((opcode & 0xff) * 4);
            if opcode & 0x800 != 0 {
                let value = self.load(bus, address, AccessWidth::Word)?;
                self.write_register(index, value);
            } else {
                self.store(bus, address, AccessWidth::Word, self.read_register(index))?;
            }
        } else if opcode & 0xf000 == 0xa000 {
            let base = if opcode & 0x800 != 0 {
                self.registers[13]
            } else {
                self.read_register(15) & !3
            };
            self.write_register(
                ((opcode >> 8) & 7) as usize,
                base.wrapping_add((opcode & 0xff) * 4),
            );
        } else if opcode & 0xff00 == 0xb000 {
            let offset = (opcode & 0x7f) * 4;
            self.registers[13] = if opcode & 0x80 != 0 {
                self.registers[13].wrapping_sub(offset)
            } else {
                self.registers[13].wrapping_add(offset)
            };
        } else if opcode & 0xf600 == 0xb400 {
            self.thumb_stack(opcode, bus)?;
        } else if opcode & 0xf000 == 0xc000 {
            // Translate to ARM IA transfer to share ARM7 empty/base-list quirks.
            let arm = 0x08a0_0000
                | ((opcode & 0x800) << 9)
                | (((opcode >> 8) & 7) << 16)
                | (opcode & 0xff);
            self.arm_block_transfer(arm, bus)?;
        } else if opcode & 0xf000 == 0xd000 {
            let condition = (opcode >> 8) & 15;
            if condition == 15 {
                self.exception(ProcessorMode::Supervisor, 8, self.address.wrapping_add(2));
            } else if condition == 14 {
                self.undefined_instruction(bus);
            } else if self.status.condition_passed(condition) {
                let offset = (opcode as u8 as i8 as i32 * 2) as u32;
                self.branch(self.read_register(15).wrapping_add(offset));
            }
        } else if opcode & 0xf800 == 0xe000 {
            let offset = (((opcode & 0x7ff) << 21) as i32 >> 20) as u32;
            self.branch(self.read_register(15).wrapping_add(offset));
        } else if opcode & 0xf800 == 0xf000 {
            let offset = (((opcode & 0x7ff) << 21) as i32 >> 9) as u32;
            self.registers[14] = self.read_register(15).wrapping_add(offset);
        } else if opcode & 0xf800 == 0xf800 {
            let target = self.registers[14].wrapping_add((opcode & 0x7ff) * 2);
            self.registers[14] = self.address.wrapping_add(2) | 1;
            self.branch(target);
        } else {
            self.undefined_instruction(bus);
        }
        Ok(())
    }

    fn thumb_alu(&mut self, opcode: u32, bus: &mut impl CpuBus) {
        let destination = (opcode & 7) as usize;
        let left = self.read_register(destination);
        let right = self.read_register(((opcode >> 3) & 7) as usize);
        let operation = (opcode >> 6) & 15;
        let mut write = true;
        let value = match operation {
            0 | 8 => {
                write = operation != 8;
                left & right
            }
            1 => left ^ right,
            2..=4 | 7 => {
                let kind = match operation {
                    2 => ShiftKind::LogicalLeft,
                    3 => ShiftKind::LogicalRight,
                    4 => ShiftKind::ArithmeticRight,
                    7 => ShiftKind::RotateRight,
                    _ => unreachable!("shift operation"),
                };
                let result = alu::shift(
                    left,
                    kind,
                    right & 0xff,
                    self.status.carry(),
                    ShiftSource::Register,
                );
                self.status.set_carry(result.carry);
                self.internal_cycles(bus, 1);
                result.value
            }
            5 => self.arithmetic_flags(alu::add_with_carry(left, right, self.status.carry())),
            6 => self.arithmetic_flags(alu::add_with_carry(left, !right, self.status.carry())),
            9 => self.arithmetic_flags(alu::add_with_carry(0, !right, true)),
            10 => {
                write = false;
                self.arithmetic_flags(alu::add_with_carry(left, !right, true))
            }
            11 => {
                write = false;
                self.arithmetic_flags(alu::add_with_carry(left, right, false))
            }
            12 => left | right,
            13 => {
                self.internal_cycles(bus, alu::multiply_cycles(left, true));
                self.status
                    .set_carry(multiply::carry(right, left, 0, MultiplyKind::Short));
                left.wrapping_mul(right)
            }
            14 => left & !right,
            15 => !right,
            _ => unreachable!("four-bit ALU operation"),
        };
        self.status.set_nz(value);
        if write {
            self.write_register(destination, value);
        }
    }

    fn thumb_high(&mut self, opcode: u32, bus: &mut impl CpuBus) {
        let destination = ((opcode & 7) | ((opcode >> 4) & 8)) as usize;
        let source = ((opcode >> 3) & 15) as usize;
        let left = self.read_register(destination);
        let right = self.read_register(source);
        match (opcode >> 8) & 3 {
            0 => self.write_register(destination, left.wrapping_add(right)),
            1 => {
                self.arithmetic_flags(alu::add_with_carry(left, !right, true));
            }
            2 => self.write_register(destination, right),
            3 if opcode & 0x80 == 0 => self.exchange(right),
            _ => self.undefined_instruction(bus), // BLX is not ARMv4T.
        }
    }

    fn thumb_register_transfer(
        &mut self,
        opcode: u32,
        bus: &mut impl CpuBus,
    ) -> Result<(), EmulatorError> {
        let index = (opcode & 7) as usize;
        let address = self
            .read_register(((opcode >> 3) & 7) as usize)
            .wrapping_add(self.read_register(((opcode >> 6) & 7) as usize));
        let operation = (opcode >> 9) & 7;
        if operation < 3 {
            let width = match operation {
                0 => AccessWidth::Word,
                1 => AccessWidth::Halfword,
                _ => AccessWidth::Byte,
            };
            self.store(bus, address, width, self.read_register(index))?;
        } else {
            let value = match operation {
                3 => self.signed_load(bus, address, 2)?,
                4 => self.load(bus, address, AccessWidth::Word)?,
                5 => self.load(bus, address, AccessWidth::Halfword)?,
                6 => self.load(bus, address, AccessWidth::Byte)?,
                7 => self.signed_load(bus, address, 3)?,
                _ => unreachable!("three-bit transfer operation"),
            };
            self.write_register(index, value);
        }
        Ok(())
    }

    fn thumb_immediate_transfer(
        &mut self,
        opcode: u32,
        bus: &mut impl CpuBus,
    ) -> Result<(), EmulatorError> {
        let width = if opcode & 0xf000 == 0x8000 {
            AccessWidth::Halfword
        } else if opcode & 0x1000 != 0 {
            AccessWidth::Byte
        } else {
            AccessWidth::Word
        };
        let offset = ((opcode >> 6) & 31) * width.bytes();
        let address = self
            .read_register(((opcode >> 3) & 7) as usize)
            .wrapping_add(offset);
        let index = (opcode & 7) as usize;
        if opcode & 0x800 != 0 {
            let value = self.load(bus, address, width)?;
            self.write_register(index, value);
        } else {
            self.store(bus, address, width, self.read_register(index))?;
        }
        Ok(())
    }

    fn thumb_stack(&mut self, opcode: u32, bus: &mut impl CpuBus) -> Result<(), EmulatorError> {
        let load = opcode & 0x800 != 0;
        let extra = opcode & 0x100 != 0;
        let list = (opcode & 0xff)
            | if extra {
                1 << if load { 15 } else { 14 }
            } else {
                0
            };
        let arm = if load {
            0x08bd_0000 | list
        } else {
            0x092d_0000 | list
        };
        self.arm_block_transfer(arm, bus)
    }
}

#[cfg(test)]
#[path = "cpu_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cpu_external_tests.rs"]
mod external_tests;

#[cfg(test)]
#[path = "cpu_upstream_tests.rs"]
mod upstream_tests;
