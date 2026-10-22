//! Deterministic Game Boy Advance emulation primitives.
//!
//! The core never opens files or consults a host clock. Hardware support is
//! incomplete; see the repository's accuracy ledger before interpreting results.

mod alu;
mod bus;
mod cpu;
mod cpu_bus;
mod emulator_error;
mod machine;
mod multiply;
mod processor_status;
mod step_trace;

pub use bus::{AccessKind, AccessWidth, Bus};
pub use cpu::Cpu;
pub use cpu_bus::CpuBus;
pub use emulator_error::EmulatorError;
pub use machine::Machine;
pub use processor_status::{ProcessorMode, ProcessorStatus};
pub use step_trace::StepTrace;
