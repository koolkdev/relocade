//! Resident DOS character drivers, distinct from machine port/MMIO devices.
//!
//! A driver owns its resources and services requests on the execution thread.
//! Registration adds a named entry; open handles retain that entry across later
//! replacements. Each request goes to its retained device; registration does not
//! automatically forward requests through other devices. Guest headers and the
//! .SYS request ABI are not implemented yet.

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

/// A device operation that may wait on the execution thread. It need not be Send.
/// Ready operations complete without yielding to the host.
pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, DosError>> + 'a>>;

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
    fn input_available(&self) -> DriverFuture<'_, bool> {
        Box::pin(async move { Ok(false) })
    }

    fn flush_input(&self) -> DriverFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    /// Reports the next character without consuming it, or None if none is ready.
    /// Servicing the request may await, but it must not wait for a new character.
    fn peek_character(&self) -> DriverFuture<'_, Option<u8>> {
        Box::pin(async { Err(DosError::AccessDenied) })
    }

    /// Reads into the supplied buffer and returns the number of bytes transferred.
    /// A short read is valid; zero means EOF (or an empty buffer). The driver
    /// owns its wait and cancellation behavior. Never return more than the buffer length.
    fn read<'a>(&'a self, _bytes: &'a mut [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async { Err(DosError::AccessDenied) })
    }

    fn output_ready(&self) -> DriverFuture<'_, bool> {
        Box::pin(async move { Ok(false) })
    }

    /// Writes bytes without DOS cooked-output processing and returns the transferred
    /// count, which may be smaller than the input length.
    fn write<'a>(&'a self, _bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        Box::pin(async { Err(DosError::AccessDenied) })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
