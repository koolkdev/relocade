//! A minimal DOS COM environment built on configured machines and host functions.
//!
//! [`load_com`] prepares one process with a PSP, empty environment, command tail,
//! stack and live INT 20h/21h vectors. Select a runtime backend when attaching the
//! returned machine. This crate has no Wasmtime, browser or host filesystem dependency.
//!
//! Console output is an uninterpreted byte stream. Inspect [`Dos::stop`] after a
//! host yield and stop executing a terminated process. Unimplemented services
//! yield a diagnostic rather than returning a fabricated success. MZ loading,
//! files, DOS allocation, BIOS/device services and protected-mode extenders are
//! outside this initial environment.
//!
//! INT 20h terminates with code zero. INT 21h implements these functions:
//!
//! | AH | Service |
//! | --- | --- |
//! | 00h | Terminate with code zero. |
//! | 02h | Append DL to console output. |
//! | 09h | Append a `$`-terminated string at DS:DX. |
//! | 25h / 35h | Set / get an interrupt vector. |
//! | 40h | Append CX bytes at DS:DX for handles 1 and 2; other handles return error 6. |
//! | 4Ch | Terminate with the exit code in AL. |
//!
//! DOS output buffers wrap their 16-bit offset within the segment. An AH=09h
//! string without `$` in one full segment yields [`DosStop::UnterminatedString`].
#![forbid(unsafe_code)]

mod loader;
mod services;

pub use loader::{load_com, LoadError};

/// Published DOS process completion or an unsupported service request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DosStop {
    Exited(u8),
    UnsupportedFunction(u8),
    UnterminatedString { segment: u16, offset: u16 },
}

/// DOS process services and buffered console output, independent of the Wasm engine.
#[derive(Default)]
pub struct Dos {
    output: Vec<u8>,
    stop: Option<DosStop>,
}

impl Dos {
    pub fn output(&self) -> &[u8] {
        &self.output
    }
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }
    pub fn stop(&self) -> Option<DosStop> {
        self.stop
    }
}
