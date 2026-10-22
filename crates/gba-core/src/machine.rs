use serde::{Deserialize, Serialize};

use crate::{Bus, Cpu, EmulatorError, StateError, StepKind, StepTrace};

/// Owns CPU and hardware state; file I/O remains the frontend's responsibility.
#[derive(Clone, Debug)]
pub struct Machine {
    cpu: Cpu,
    bus: Bus,
    fault: Option<EmulatorError>,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    cpu: Cpu,
    bus: Bus,
    #[serde(deserialize_with = "crate::state::deserialize_bounded_vec")]
    registers: Vec<u8>,
}

impl Machine {
    /// Writes a complete, versioned state bound to this machine's ROM image.
    pub fn save_state(&self) -> Result<Vec<u8>, StateError> {
        if self.fault.is_some() {
            return Err(StateError::FaultedMachine);
        }
        let (bus, registers) = self.bus.snapshot();
        let snapshot = Snapshot {
            cpu: self.cpu.clone(),
            bus,
            registers,
        };
        let payload = postcard::to_allocvec(&snapshot).map_err(|_| StateError::InvalidData)?;
        crate::state::encode(self.bus.rom_hash(), &payload)
    }

    /// Restores a complete state only after validating its container, ROM,
    /// decoded data, and memory/device invariants. On error this machine is unchanged.
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), StateError> {
        let payload = crate::state::decode(bytes, &self.bus.rom_hash())?;
        let (snapshot, remaining) =
            postcard::take_from_bytes::<Snapshot>(payload).map_err(|_| StateError::InvalidData)?;
        if !remaining.is_empty() || !snapshot.cpu.valid_snapshot() {
            return Err(StateError::InvalidData);
        }
        let Snapshot {
            cpu,
            bus,
            registers,
        } = snapshot;
        self.bus.restore_snapshot(bus, &registers)?;
        self.cpu = cpu;
        self.fault = None;
        Ok(())
    }

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

    /// Drains stereo signed-16 PCM frames at AUDIO_SAMPLE_RATE (left, right).
    /// The core retains at most half a second; slow consumers discard oldest data.
    pub fn take_audio(&mut self) -> Vec<[i16; 2]> {
        self.bus.hardware.apu.take_samples()
    }

    /// Total PCM frames generated since reset, including discarded output.
    #[must_use]
    pub fn audio_frames(&self) -> u64 {
        self.bus.hardware.apu.generated
    }

    /// PCM frames discarded because a frontend did not drain the bounded queue.
    #[must_use]
    pub fn audio_dropped(&self) -> u64 {
        self.bus.hardware.apu.dropped
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
    fn save_state_restores_full_machine_and_deterministic_continuation() {
        let mut bios = vec![0; 0x4000];
        for opcode in bios.chunks_exact_mut(4) {
            opcode.copy_from_slice(&0xeaff_fffeu32.to_le_bytes());
        }
        let rom = vec![0x42; 192];
        let mut saved = Machine::try_new(&bios, &rom).expect("machine");
        saved.run_frame().expect("first frame");
        saved
            .bus
            .write(
                0x0200_0100,
                crate::AccessWidth::Word,
                0x1234_5678,
                crate::AccessKind::Nonsequential,
            )
            .expect("EWRAM write");
        let state = saved.save_state().expect("state file");
        let expected_clocks = saved.clocks();
        let expected_frames = saved.frames();
        let mut expected = saved.clone();

        let mut restored = Machine::try_new(&bios, &rom).expect("fresh machine");
        restored.load_state(&state).expect("load state");
        assert_eq!(restored.clocks(), expected_clocks);
        assert_eq!(restored.frames(), expected_frames);
        assert_eq!(
            restored
                .bus
                .read(
                    0x0200_0100,
                    crate::AccessWidth::Word,
                    crate::AccessKind::Nonsequential,
                )
                .expect("EWRAM read"),
            0x1234_5678
        );
        // Restore the read's bus-time side effect equally before comparing continuation.
        saved
            .bus
            .read(
                0x0200_0100,
                crate::AccessWidth::Word,
                crate::AccessKind::Nonsequential,
            )
            .expect("matching EWRAM read");
        expected
            .bus
            .read(
                0x0200_0100,
                crate::AccessWidth::Word,
                crate::AccessKind::Nonsequential,
            )
            .expect("matching EWRAM read");
        expected.run_frame().expect("matching continuation");
        restored.run_frame().expect("restored continuation");
        assert_eq!(restored.clocks(), expected.clocks());
        assert_eq!(restored.frames(), expected.frames());
        assert_eq!(restored.cpu.registers(), expected.cpu.registers());
        assert_eq!(restored.screen(), expected.screen());
        assert_eq!(restored.take_audio(), expected.take_audio());
    }

    #[test]
    fn invalid_state_is_rejected_without_mutating_and_other_rom_is_rejected() {
        let bios = vec![0; 0x4000];
        let rom = vec![0x11; 192];
        let mut source = Machine::try_new(&bios, &rom).expect("machine");
        let state = source.save_state().expect("state");
        let mut other = Machine::try_new(&bios, &[0x22; 192]).expect("other ROM");
        let clocks = other.clocks();
        assert_eq!(
            other.load_state(&state),
            Err(crate::StateError::DifferentRom)
        );
        assert_eq!(other.clocks(), clocks);
        let mut corrupt = state;
        *corrupt.last_mut().expect("payload") ^= 1;
        assert_eq!(
            source.load_state(&corrupt),
            Err(crate::StateError::InvalidData)
        );
        let malformed = crate::state::encode(source.bus.rom_hash(), &[0xff])
            .expect("valid envelope around invalid payload");
        assert_eq!(
            source.load_state(&malformed),
            Err(crate::StateError::InvalidData)
        );
        assert_eq!(source.clocks(), 0);
    }

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
