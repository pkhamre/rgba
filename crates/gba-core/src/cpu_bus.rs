use crate::{AccessKind, AccessWidth, EmulatorError};

/// CPU-facing bus contract, allowing independent transaction-vector validation.
///
/// Implementations own clock advancement. Reads/writes perform physical alignment;
/// the CPU performs ARM7 load rotation and sign extension. Device faults are
/// terminal for the instruction being executed.
pub trait CpuBus {
    /// Elapsed master clocks.
    fn clocks(&self) -> u64;
    /// Advance clocks without an external CPU memory request.
    fn internal_cycles(&mut self, count: u64);
    /// Update executing PC for BIOS access protection and tracing.
    fn set_execution_address(&mut self, address: u32);
    /// Fetch an instruction without conflating it with a data read.
    fn fetch(
        &mut self,
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
    ) -> Result<u32, EmulatorError>;
    /// Read physical data, aligned according to the bus implementation.
    fn read(
        &mut self,
        address: u32,
        width: AccessWidth,
        kind: AccessKind,
    ) -> Result<u32, EmulatorError>;
    /// Write physical data, aligned according to the bus implementation.
    fn write(
        &mut self,
        address: u32,
        width: AccessWidth,
        value: u32,
        kind: AccessKind,
    ) -> Result<(), EmulatorError>;
}
