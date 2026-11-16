//! Numbered LPT drivers and the byte output shared with the PRN driver.

use super::{CharacterDriver, DriverFuture};
use crate::DosError;
use futures_io::AsyncWrite;
use relocade_parallel::centronics;
use std::{cell::RefCell, future::poll_fn, pin::Pin};

pub(crate) const LPT_NAMES: [[u8; 8]; 3] = [*b"LPT1    ", *b"LPT2    ", *b"LPT3    "];

pub(crate) struct Lpt {
    name: [u8; 8],
    output: ParallelOutput,
}

impl Lpt {
    pub(crate) fn new(number: u8, output: centronics::Output) -> Result<Self, DosError> {
        let name = number
            .checked_sub(1)
            .and_then(|index| LPT_NAMES.get(usize::from(index)))
            .copied()
            .ok_or(DosError::InvalidData)?;
        Ok(Self {
            name,
            output: ParallelOutput::new(output),
        })
    }
}

impl CharacterDriver for Lpt {
    fn name(&self) -> [u8; 8] {
        self.name
    }

    fn output_ready(&self) -> DriverFuture<'_, bool> {
        Box::pin(async { Ok(self.output.ready()) })
    }

    fn write<'a>(&'a self, bytes: &'a [u8]) -> DriverFuture<'a, usize> {
        Box::pin(self.output.write(bytes))
    }
}

/// Owns a driver's connection and maps stream progress and failures to DOS.
pub(super) struct ParallelOutput(RefCell<centronics::Output>);

impl ParallelOutput {
    pub(super) fn new(output: centronics::Output) -> Self {
        Self(RefCell::new(output))
    }

    pub(super) fn ready(&self) -> bool {
        self.0.borrow().ready()
    }

    pub(super) async fn write(&self, bytes: &[u8]) -> Result<usize, DosError> {
        let mut written = 0;
        while written < bytes.len() {
            let result = poll_fn(|cx| {
                Pin::new(&mut *self.0.borrow_mut()).poll_write(cx, &bytes[written..])
            })
            .await;
            match result {
                Ok(0) | Err(_) if written == 0 => return Err(DosError::WriteFault),
                Ok(0) | Err(_) => break,
                Ok(count) => written += count,
            }
        }
        Ok(written)
    }
}
