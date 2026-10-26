use super::output::CharacterOutput;
use crate::{error::DosError, files::CharacterInput, DosServices};
use relocade_machine::CpuContext;

/// A line being edited, including its final carriage return once complete.
pub(super) struct LineInput {
    bytes: Vec<u8>,
    capacity: u8,
    echo: CharacterOutput,
    extended_key: bool,
    first: bool,
}

impl LineInput {
    /// Capacity includes the carriage return and must be nonzero.
    pub(super) fn new(capacity: u8, echo: CharacterOutput) -> Self {
        Self {
            bytes: Vec::new(),
            capacity,
            echo,
            extended_key: false,
            first: true,
        }
    }

    /// Keeps editing locals without holding a DOS-state borrow while awaiting input.
    pub(super) async fn read(
        mut self,
        call: &mut CpuContext<'_, DosServices>,
        services: &DosServices,
        input: &CharacterInput,
    ) -> Result<Vec<u8>, DosError> {
        loop {
            let byte = input.read_line_character().await?;
            if self.accept(call, services, byte) {
                return Ok(self.bytes);
            }
        }
    }

    fn accept(
        &mut self,
        call: &mut CpuContext<'_, DosServices>,
        services: &DosServices,
        byte: u8,
    ) -> bool {
        if self.extended_key {
            self.extended_key = false;
            return false;
        }
        let first = std::mem::replace(&mut self.first, false);
        match byte {
            0 => self.extended_key = true,
            b'\r' => {
                self.bytes.push(byte);
                self.echo(call, services, b"\r");
                return true;
            }
            // Discard a leading LF left over from redirected CRLF input.
            b'\n' => {
                if !first {
                    self.echo(call, services, b"\r\n");
                }
            }
            8 | 127 => {
                if let Some(byte) = self.bytes.pop() {
                    self.echo(call, services, b"\x08 \x08");
                    if byte < b' ' && byte != b'\t' {
                        self.echo(call, services, b"\x08 \x08");
                    }
                }
            }
            _ if self.bytes.len() == usize::from(self.capacity) - 1 => {
                self.echo(call, services, b"\x07");
            }
            _ => {
                self.bytes.push(byte);
                if byte < b' ' && byte != b'\t' {
                    self.echo(call, services, &[b'^', byte + b'@']);
                } else {
                    self.echo(call, services, &[byte]);
                }
            }
        }
        false
    }

    fn echo(&self, call: &mut CpuContext<'_, DosServices>, services: &DosServices, bytes: &[u8]) {
        self.echo.write_cooked(call, services, bytes);
    }
}
