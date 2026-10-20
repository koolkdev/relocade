//! Character input through standard handles and the console.

use super::{Device, Files, HandleType};
use crate::error::DosError;
use std::future::{pending, Future};

impl Files {
    /// Flush the device queue without changing file cursors, buffered lines or EOF.
    pub(crate) fn flush_input(&mut self, handle: u16) -> Result<(), DosError> {
        match self.handle_type(handle)? {
            HandleType::Device(Device::Console(_)) => {
                self.stdio.input.discard_queued();
            }
            HandleType::File | HandleType::Device(_) => {}
        }
        Ok(())
    }

    /// Reads one raw character from a console or redirected file handle.
    pub(crate) fn read_character(&mut self, handle: u16) -> Result<Option<u8>, DosError> {
        match self.readable_handle_type(handle)? {
            HandleType::Device(Device::Console(_)) => Ok(self.stdio.input.try_read_character()),
            // The NUL driver's empty character response is returned as AL=0.
            HandleType::Device(Device::Null) => Ok(Some(0)),
            HandleType::File => {
                let mut byte = [0];
                Ok((self.read(handle, &mut byte)? != 0).then_some(byte[0]))
            }
            HandleType::Device(Device::Auxiliary | Device::Printer) => Err(DosError::AccessDenied),
        }
    }

    /// Waits after an empty character read. Redirected EOF cannot consume console input.
    pub(crate) fn wait_for_character(
        &self,
        handle: u16,
    ) -> Result<impl Future<Output = u8> + 'static, DosError> {
        let input = match self.readable_handle_type(handle)? {
            HandleType::Device(Device::Console(_)) => Some(self.stdio.input.clone()),
            HandleType::File | HandleType::Device(Device::Null) => None,
            HandleType::Device(Device::Auxiliary | Device::Printer) => {
                return Err(DosError::AccessDenied);
            }
        };
        Ok(async move {
            match input {
                Some(input) => input.read_character().await,
                // Files cannot notify growth; NUL cannot produce a completed input line.
                None => pending().await,
            }
        })
    }
}
