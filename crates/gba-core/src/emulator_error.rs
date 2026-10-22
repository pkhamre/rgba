use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

/// Invalid assets or hardware behavior not implemented by the current core.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EmulatorError {
    /// The GBA BIOS must be exactly 16 KiB.
    InvalidBiosSize(usize),
    /// Cartridge images must contain a header and fit the 32 MiB address window.
    InvalidRomSize(usize),
    /// An instruction encoding is not implemented; execution must not continue.
    UnsupportedInstruction {
        /// Executing instruction address.
        address: u32,
        /// Raw instruction bits.
        opcode: u32,
        /// Whether the instruction was decoded as Thumb.
        thumb: bool,
    },
    /// The requested device register has no implementation yet.
    UnsupportedIo {
        /// Device register address.
        address: u32,
        /// True for writes, false for reads.
        write: bool,
    },
    /// Cartridge backup or GPIO hardware has not been implemented yet.
    UnsupportedCartridge {
        /// Cartridge bus address.
        address: u32,
        /// True for writes, false for reads.
        write: bool,
    },
    /// Backup data length does not match the detected cartridge type.
    InvalidSaveSize {
        /// Required cartridge backup length.
        expected: usize,
        /// Supplied backup length.
        actual: usize,
    },
    /// Software selected a reserved processor mode.
    InvalidProcessorMode(u32),
}

impl fmt::Display for EmulatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBiosSize(size) => {
                write!(formatter, "BIOS must be 16384 bytes, got {size}")
            }
            Self::InvalidRomSize(size) => {
                write!(formatter, "ROM must be 192..=33554432 bytes, got {size}")
            }
            Self::UnsupportedInstruction {
                address,
                opcode,
                thumb,
            } => write!(
                formatter,
                "unsupported {} instruction {opcode:08x} at {address:08x}",
                if *thumb { "Thumb" } else { "ARM" }
            ),
            Self::UnsupportedIo { address, write } => write!(
                formatter,
                "unimplemented I/O {} at {address:08x}",
                if *write { "write" } else { "read" }
            ),
            Self::UnsupportedCartridge { address, write } => write!(
                formatter,
                "unimplemented cartridge {} at {address:08x}",
                if *write { "write" } else { "read" }
            ),
            Self::InvalidSaveSize { expected, actual } => {
                write!(formatter, "save must be {expected} bytes, got {actual}")
            }
            Self::InvalidProcessorMode(mode) => {
                write!(formatter, "reserved processor mode {mode:02x}")
            }
        }
    }
}

impl Error for EmulatorError {}
