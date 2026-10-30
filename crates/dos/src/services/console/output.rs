//! Output routing and cooked characters over retained console resources.

use crate::{
    error::DosError,
    files::{Device, Handle},
    stdio::{OutputStream, Stdio},
};
use relocade_machine::{IoBus, Memory};

/// Console services need memory and devices, without any CPU access.
#[derive(Clone, Copy)]
pub(crate) struct Console<'a> {
    pub(crate) stdio: &'a Stdio,
    pub(crate) memory: &'a Memory,
    pub(crate) io: &'a IoBus,
}

impl<'a> Console<'a> {
    pub(crate) fn write(&self, stream: OutputStream, bytes: &[u8]) {
        self.stdio.output(stream).extend(bytes);
        relocade_bios::write_text(&mut self.memory.view().with_io(self.io), self.io, bytes);
    }

    pub(crate) fn output(self, destination: Result<Handle, DosError>) -> CharacterOutput<'a> {
        CharacterOutput {
            console: self,
            destination: Destination::Handle(destination),
        }
    }

    pub(crate) fn stream(self, stream: OutputStream) -> CharacterOutput<'a> {
        CharacterOutput {
            console: self,
            destination: Destination::Console(stream),
        }
    }
}

enum Destination {
    Handle(Result<Handle, DosError>),
    Console(OutputStream),
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
        let (byte, count) = self.console.stdio.cook_character(byte);
        let bytes = [byte; 8];
        let _ = self.write(&bytes[..usize::from(count)]);
        byte
    }

    /// Character output preserves console EOF; only handle writes clear it.
    pub(crate) fn write(&self, bytes: &[u8]) -> Result<(), DosError> {
        match &self.destination {
            Destination::Console(stream) => self.console.write(*stream, bytes),
            Destination::Handle(Err(error)) => return Err(*error),
            Destination::Handle(Ok(Handle::File(file))) => {
                if !bytes.is_empty() {
                    file.write(bytes)?;
                }
            }
            Destination::Handle(Ok(Handle::Device(device))) => match device.kind() {
                Device::Console(stream) => self.console.write(stream, bytes),
                Device::Null => {}
                Device::Auxiliary | Device::Printer => return Err(DosError::AccessDenied),
            },
        }
        Ok(())
    }
}
