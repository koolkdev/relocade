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
    /// Cooked output expands tabs, tracks its column and ignores write errors.
    /// Returns the last output byte (a space for a tab).
    pub(crate) fn write_cooked(&self, byte: u8) -> u8 {
        let column = || self.device().and_then(|device| device.output_column());
        let (byte, count) = self.console.stdio.cook_character(byte, column());
        let bytes = [byte; 8];
        let _ = self.write(&bytes[..usize::from(count)]);
        // Cursor controls can move by a different amount than their byte length.
        if let Some(column) = column() {
            self.console.stdio.set_output_column(column);
        }
        byte
    }

    fn device(&self) -> Option<&CharacterDevice> {
        match &self.destination {
            Destination::Device(device) => Some(device),
            Destination::Handle(Ok(Handle::Device(handle))) => Some(&handle.device),
            _ => None,
        }
    }

    /// Character output preserves console EOF; only handle writes clear it.
    pub(crate) fn write(&self, bytes: &[u8]) -> Result<(), DosError> {
        match &self.destination {
            Destination::Device(device) => device.write(bytes)?,
            Destination::Handle(Err(error)) => return Err(*error),
            Destination::Handle(Ok(Handle::File(file))) => {
                if !bytes.is_empty() {
                    file.write(bytes)?;
                }
            }
            Destination::Handle(Ok(Handle::Device(handle))) => {
                handle.device.write(bytes)?;
                if handle.device.is_console() {
                    self.console.stdio.output(handle.stream).extend(bytes);
                }
            }
        }
        Ok(())
    }
}
