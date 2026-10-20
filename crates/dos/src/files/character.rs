//! Character I/O through standard handles and the console.

use super::{handles::OpenHandle, Files, HandleType};
use crate::{error::DosError, stdio::OutputStream};
use std::future::{pending, Future};

#[derive(Clone, Copy)]
pub(crate) enum CharacterOutput {
    Stdout,
    Console(OutputStream),
}

impl Files {
    /// DOS cooked output expands tabs, tracks its column and ignores write errors.
    /// Returns the last output byte (a space for a tab).
    pub(crate) fn write_cooked_character(&mut self, output: CharacterOutput, byte: u8) -> u8 {
        let (byte, count) = if byte == b'\t' {
            (b' ', 8 - (self.stdio.output_column & 7))
        } else {
            (byte, 1)
        };
        let column = &mut self.stdio.output_column;
        match byte {
            b'\r' => *column = 0,
            b'\x08' => *column = column.wrapping_sub(1),
            b' '..=b'~' | 0x80..=0xff => *column = column.wrapping_add(count),
            _ => {}
        }
        let bytes = [byte; 8];
        let bytes = &bytes[..usize::from(count)];
        match output {
            CharacterOutput::Stdout => {
                let _ = self.write_characters(1, bytes);
            }
            CharacterOutput::Console(stream) => self.stdio.output(stream).extend(bytes),
        }
        byte
    }

    /// Reads one raw character from a console or redirected file handle.
    pub(crate) fn read_character(&mut self, handle: u16) -> Result<Option<u8>, DosError> {
        match self.handle_type(handle)? {
            HandleType::ConsoleInput => Ok(self.stdio.input.try_read()),
            HandleType::File => {
                let mut byte = [0];
                Ok((self.read(handle, &mut byte)? != 0).then_some(byte[0]))
            }
            HandleType::ConsoleOutput | HandleType::Auxiliary | HandleType::Printer => {
                Err(DosError::AccessDenied)
            }
        }
    }

    /// Waits after an empty character read. Redirected EOF cannot consume console input.
    pub(crate) fn wait_for_character(
        &self,
        handle: u16,
    ) -> Result<impl Future<Output = u8> + 'static, DosError> {
        let input = match self.handle_type(handle)? {
            HandleType::ConsoleInput => Some(self.stdio.input.read_character()),
            HandleType::File => None,
            HandleType::ConsoleOutput | HandleType::Auxiliary | HandleType::Printer => {
                return Err(DosError::AccessDenied);
            }
        };
        Ok(async move {
            match input {
                Some(input) => input.await,
                // Character reads cannot report EOF. File growth notifications are unsupported.
                None => pending().await,
            }
        })
    }

    /// Character output preserves console EOF; only AH=40h handle writes clear it.
    pub(crate) fn write_characters(&mut self, handle: u16, bytes: &[u8]) -> Result<(), DosError> {
        match self.handle(handle)? {
            OpenHandle::ConsoleInput { .. } => {
                self.stdio.output(OutputStream::Stdout).extend(bytes)
            }
            OpenHandle::ConsoleOutput(stream) => self.stdio.output(*stream).extend(bytes),
            OpenHandle::File(_) => {
                if !bytes.is_empty() {
                    self.write(handle, bytes)?;
                }
            }
            OpenHandle::Auxiliary | OpenHandle::Printer => return Err(DosError::AccessDenied),
        }
        Ok(())
    }
}
