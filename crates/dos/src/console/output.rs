//! Output routing and cooked characters over retained console resources.

use crate::{
    devices::CharacterDevice,
    error::DosError,
    files::{DeviceHandle, Handle},
    Console,
};
use std::rc::Rc;

impl<'a> Console<'a> {
    pub(crate) fn output(self, destination: Result<Handle, DosError>) -> CharacterOutput<'a> {
        CharacterOutput {
            console: self,
            destination: Destination::Handle(destination),
        }
    }

    pub(crate) fn device_output(self, handle: Rc<DeviceHandle>) -> CharacterOutput<'a> {
        self.output(Ok(Handle::Device(handle)))
    }

    pub(crate) fn direct_output(self, device: Rc<CharacterDevice>) -> CharacterOutput<'a> {
        CharacterOutput {
            console: self,
            destination: Destination::Device(device),
        }
    }
}

enum Destination {
    Device(Rc<CharacterDevice>),
    Handle(Result<Handle, DosError>),
}

/// An output destination resolved before a service waits for input.
pub(crate) struct CharacterOutput<'a> {
    console: Console<'a>,
    destination: Destination,
}

impl CharacterOutput<'_> {
    /// Expands tabs, tracks the resulting column, and returns device errors.
    pub(crate) async fn write_cooked(&self, byte: u8) -> Result<(), DosError> {
        let (byte, count) = self.console.stdio.cook_character(byte);
        let bytes = [byte; 8];
        self.write_all(&bytes[..usize::from(count)]).await
    }

    /// Character output preserves console EOF; only handle writes clear it.
    pub(crate) async fn write(&self, bytes: &[u8]) -> Result<usize, DosError> {
        match &self.destination {
            Destination::Device(device) => device.write(bytes).await,
            Destination::Handle(Err(error)) => Err(*error),
            Destination::Handle(Ok(Handle::File(file))) => {
                if bytes.is_empty() {
                    Ok(0)
                } else {
                    file.write(bytes)
                }
            }
            Destination::Handle(Ok(Handle::Device(handle))) => {
                let count = handle.device.write(bytes).await?;
                if handle.device.is_console() {
                    self.console
                        .stdio
                        .output(handle.stream)
                        .extend(&bytes[..count]);
                }
                Ok(count)
            }
        }
    }

    /// Completes cooked output even when a driver accepts only part of a write.
    pub(crate) async fn write_all(&self, mut bytes: &[u8]) -> Result<(), DosError> {
        while !bytes.is_empty() {
            let count = self.write(bytes).await?;
            if count == 0 {
                return Err(DosError::WriteFault);
            }
            bytes = &bytes[count..];
        }
        Ok(())
    }
}
