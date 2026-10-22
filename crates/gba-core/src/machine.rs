use crate::{Bus, Cpu, EmulatorError, StepTrace};

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

    /// Executes one instruction. Errors latch until a new machine is created.
    ///
    /// A device error may occur after partial effects, so resuming after failure
    /// would be incorrect. Repeated calls return the same fault without mutation.
    pub fn step(&mut self) -> Result<StepTrace, EmulatorError> {
        if let Some(error) = self.fault {
            return Err(error);
        }
        match self.cpu.step(&mut self.bus) {
            Ok(trace) => Ok(trace),
            Err(error) => {
                self.fault = Some(error);
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Machine;

    #[test]
    fn device_fault_is_terminal_and_does_not_advance_on_retry() {
        let mut bios = vec![0; 0x4000];
        for (index, opcode) in [0xe3a0_0301u32, 0xe590_1000].into_iter().enumerate() {
            bios[index * 4..index * 4 + 4].copy_from_slice(&opcode.to_le_bytes());
        }
        let mut machine = Machine::try_new(&bios, &[0; 192]).expect("valid images");
        machine.step().expect("MOV r0, #0x04000000");
        let error = machine
            .step()
            .expect_err("display I/O is not yet supported");
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
