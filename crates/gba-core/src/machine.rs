use crate::{Bus, Cpu, EmulatorError, StepKind, StepTrace};

/// Owns CPU and hardware state; file I/O remains the frontend's responsibility.
#[derive(Clone, Debug)]
pub struct Machine {
    cpu: Cpu,
    bus: Bus,
    fault: Option<EmulatorError>,
}

impl Machine {
    /// Creates a powered-on machine at BIOS address zero.
    ///
    /// ```
    /// use gba_core::Machine;
    ///
    /// // Synthetic images suffice for constructing a test machine.
    /// let machine = Machine::try_new(&[0; 16 * 1024], &[0; 192])?;
    /// assert_eq!(machine.cpu().instruction_address(), 0);
    /// assert_eq!(machine.clocks(), 0);
    /// # Ok::<(), gba_core::EmulatorError>(())
    /// ```
    pub fn try_new(bios: &[u8], rom: &[u8]) -> Result<Self, EmulatorError> {
        Ok(Self {
            cpu: Cpu::default(),
            bus: Bus::try_new(bios, rom)?,
            fault: None,
        })
    }

    /// Read-only debugger view of CPU state.
    #[must_use]
    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    /// Elapsed emulated master clocks.
    #[must_use]
    pub fn clocks(&self) -> u64 {
        self.bus.clocks()
    }

    /// Most recently rendered 240×160 pixels as 0x00RRGGBB values.
    #[must_use]
    pub fn screen(&self) -> &[u32] {
        &self.bus.ppu.pixels
    }

    /// Cartridge backup bytes (Flash or SRAM), not an emulator save state.
    #[must_use]
    pub fn save_data(&self) -> &[u8] {
        &self.bus.cartridge.save
    }

    /// Whether emulated cartridge writes changed backup data.
    #[must_use]
    pub fn save_dirty(&self) -> bool {
        self.bus.cartridge.dirty
    }

    /// Monotonic cartridge-write counter for quiet-period save scheduling.
    #[must_use]
    pub fn save_revision(&self) -> u64 {
        self.bus.cartridge.revision
    }

    /// Called by the frontend only after a successful atomic save.
    pub fn mark_save_clean(&mut self) {
        self.bus.cartridge.dirty = false;
    }

    /// Loads cartridge backup data with strict size validation.
    pub fn load_save(&mut self, bytes: &[u8]) -> Result<(), EmulatorError> {
        let expected = self.save_data().len();
        if bytes.len() != expected {
            return Err(EmulatorError::InvalidSaveSize {
                expected,
                actual: bytes.len(),
            });
        }
        self.bus.cartridge.save.copy_from_slice(bytes);
        self.mark_save_clean();
        Ok(())
    }

    /// Seeds cartridge RTC from a frontend-supplied Unix timestamp (UTC seconds).
    pub fn set_rtc_timestamp(&mut self, seconds: u64) {
        self.bus.cartridge.set_timestamp(seconds);
    }

    /// Completed hardware display frames.
    #[must_use]
    pub fn frames(&self) -> u64 {
        self.bus.hardware.frames
    }

    /// Sets held buttons: bits 0..9 are A, B, Select, Start, Right, Left, Up,
    /// Down, R, L. A one means pressed; hardware KEYINPUT is active-low.
    pub fn set_keys(&mut self, pressed: u16) {
        self.bus.hardware.set_keys(pressed & 0x3ff);
    }

    /// Advances through one hardware frame, including CPU, DMA and halt time.
    pub fn run_frame(&mut self) -> Result<(), EmulatorError> {
        let frame = self.frames();
        while self.frames() == frame {
            self.step()?;
        }
        Ok(())
    }

    /// Executes one CPU activity. Errors latch until a new machine is created.
    ///
    /// A device error may occur after partial effects, so resuming after failure
    /// would be incorrect. Repeated calls return the same fault without mutation.
    pub fn step(&mut self) -> Result<StepTrace, EmulatorError> {
        if let Some(error) = self.fault {
            return Err(error);
        }
        match self.step_inner() {
            Ok(trace) => Ok(trace),
            Err(error) => {
                self.fault = Some(error);
                Err(error)
            }
        }
    }
    fn step_inner(&mut self) -> Result<StepTrace, EmulatorError> {
        self.bus.service_dma()?;
        let clock_before = self.clocks();
        if self.bus.hardware.halted && self.bus.hardware.wake_pending() {
            self.bus.hardware.halted = false;
        }
        let kind = if self.bus.hardware.irq_pending() && self.cpu.take_irq(&mut self.bus)? {
            Some(StepKind::Irq)
        } else if self.bus.hardware.halted {
            self.bus.advance(self.bus.hardware.next_event());
            Some(StepKind::Halt)
        } else {
            None
        };
        if let Some(kind) = kind {
            return Ok(StepTrace {
                kind,
                address: self.cpu.instruction_address(),
                opcode: 0,
                thumb: self.cpu.status().thumb(),
                condition_passed: false,
                clock_before,
                clock_after: self.clocks(),
                registers: self.cpu.registers(),
                status: self.cpu.status().bits(),
            });
        }
        self.cpu.step(&mut self.bus)
    }
}

#[cfg(test)]
mod tests {
    use super::Machine;

    #[test]
    fn frame_clock_runs_during_halt_and_irq_vectors() {
        let mut bios = [0; 0x4000];
        bios[..4].copy_from_slice(&0xe321_f01fu32.to_le_bytes());
        let mut machine = Machine::try_new(&bios, &[0; 192]).expect("synthetic machine");
        machine.step().expect("MSR enters System with IRQ enabled");
        machine.bus.hardware.set_half(0x200, 1);
        machine.bus.hardware.set_half(0x202, 1);
        machine.bus.hardware.set_half(0x208, 1);
        let trace = machine.step().expect("IRQ entry");
        assert_eq!(trace.kind, crate::StepKind::Irq);
        assert_eq!(machine.cpu.instruction_address(), 0x18);
        machine.bus.hardware.set_half(0x200, 0);
        machine.bus.hardware.halted = true;
        machine.run_frame().expect("HALT must not stop peripherals");
        assert_eq!(machine.frames(), 1);
        assert_eq!(machine.clocks(), 280896);
    }

    #[test]
    fn device_fault_is_terminal_and_does_not_advance_on_retry() {
        let mut bios = vec![0; 0x4000];
        for (index, opcode) in [0xe3a0_0301u32, 0xe590_1400].into_iter().enumerate() {
            bios[index * 4..index * 4 + 4].copy_from_slice(&opcode.to_le_bytes());
        }
        let mut machine = Machine::try_new(&bios, &[0; 192]).expect("valid images");
        machine.step().expect("MOV r0, #0x04000000");
        let error = machine
            .step()
            .expect_err("unmapped I/O remains an explicit fault");
        let clocks = machine.clocks();
        let registers = machine.cpu().registers();
        assert_eq!(machine.step(), Err(error));
        assert_eq!(
            machine.clocks(),
            clocks,
            "retry must not mutate a failed machine"
        );
        assert_eq!(machine.cpu().registers(), registers);
    }
}
