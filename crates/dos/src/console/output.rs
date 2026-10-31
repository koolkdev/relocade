//! Output routing and cooked characters over retained console resources.

use crate::{
    error::DosError,
    files::{Device, Handle},
    stdio::OutputStream,
    Console,
};

impl<'a> Console<'a> {
    pub(crate) fn write_stream(&self, stream: OutputStream, bytes: &[u8]) {
        self.stdio.output(stream).extend(bytes);
        self.write_video(bytes);
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

    pub(crate) fn screen(self) -> CharacterOutput<'a> {
        CharacterOutput {
            console: self,
            destination: Destination::Screen,
        }
    }
}

enum Destination {
    Screen,
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
            Destination::Screen => self.console.write_video(bytes),
            Destination::Console(stream) => self.console.write_stream(*stream, bytes),
            Destination::Handle(Err(error)) => return Err(*error),
            Destination::Handle(Ok(Handle::File(file))) => {
                if !bytes.is_empty() {
                    file.write(bytes)?;
                }
            }
            Destination::Handle(Ok(Handle::Device(device))) => match device.kind() {
                Device::Console(stream) => self.console.write_stream(stream, bytes),
                Device::Null => {}
                Device::Auxiliary | Device::Printer => return Err(DosError::AccessDenied),
            },
        }
        Ok(())
    }
}
