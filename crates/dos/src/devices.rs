//! Resident DOS character drivers, distinct from machine port/MMIO devices.
//!
//! A driver owns its resources and services requests on the execution thread.
//! Registration adds a named entry; open handles retain that entry across later
//! replacements. Guest headers and the .SYS request ABI are not implemented yet.

mod ansi;
mod builtin;
mod chain;

use crate::DosError;
pub use ansi::AnsiConsole;
pub(crate) use builtin::{BiosConsole, NullDevice, Unavailable};
pub use chain::CharacterDevice;
pub(crate) use chain::DeviceChain;
use std::{future::Future, pin::Pin};

/// DOS character-driver attributes, also reported by IOCTL.
pub mod attributes {
    /// Character device rather than a block device.
    pub const CHARACTER: u16 = 0x8000;
    /// Character device providing standard console input and output.
    pub const CONSOLE: u16 = CHARACTER | 3;
    /// Null device; handle reads return EOF.
    pub const NULL: u16 = CHARACTER | 4;
}

/// A character read that may wait on the execution thread. It need not be Send.
pub type ReadFuture<'a> = Pin<Box<dyn Future<Output = Result<u8, DosError>> + 'a>>;

/// A resident DOS character driver. Methods take shared access so a pending read
/// does not prevent output or status requests. Release mutable state borrows
/// before awaiting. The default operations describe an unavailable device.
pub trait CharacterDriver {
    /// DOS name, padded with spaces to eight bytes. Captured at installation.
    fn name(&self) -> [u8; 8];

    /// DOS driver attributes, including [`attributes::CHARACTER`]. Captured at installation.
    fn attributes(&self) -> u16 {
        attributes::CHARACTER
    }

    /// Whether a character read can complete immediately.
    fn input_available(&self) -> bool {
        false
    }

    fn flush_input(&self) {}

    /// Takes a character without waiting, or returns None when input is pending.
    fn try_read_character(&self) -> Result<Option<u8>, DosError> {
        Err(DosError::AccessDenied)
    }

    /// Waits for one character. The driver owns its wait and cancellation behavior.
    fn read_character(&self) -> ReadFuture<'_> {
        Box::pin(std::future::ready(Err(DosError::AccessDenied)))
    }

    fn output_ready(&self) -> bool {
        false
    }

    /// Writes all supplied bytes, without DOS cooked-output processing.
    fn write(&self, _bytes: &[u8]) -> Result<(), DosError> {
        Err(DosError::AccessDenied)
    }

    /// An authoritative cursor column for DOS tab expansion. None uses DOS's
    /// byte-counted column. Drivers interpreting cursor controls can override it.
    fn output_column(&self) -> Option<u8> {
        None
    }
}

#[cfg(test)]
mod tests;
