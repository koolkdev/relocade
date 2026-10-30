use super::{input, output::CharacterOutput};
use crate::{error::DosError, files::Handle};
use relocade_bios::Keyboard;

/// Edits a line with capacity for its final carriage return.
pub(in crate::services) async fn read_line(
    input: &Keyboard,
    source: &Handle,
    capacity: u8,
    echo: &CharacterOutput<'_>,
) -> Result<Vec<u8>, DosError> {
    let mut editor = LineEditor {
        bytes: Vec::new(),
        capacity,
        extended_key: false,
        first: true,
    };
    loop {
        let byte = input::read_character(source, input).await?;
        if editor.accept(echo, byte) {
            return Ok(editor.bytes);
        }
    }
}

/// A line being edited, including its final carriage return once complete.
struct LineEditor {
    bytes: Vec<u8>,
    capacity: u8,
    extended_key: bool,
    first: bool,
}

impl LineEditor {
    fn accept(&mut self, output: &CharacterOutput<'_>, byte: u8) -> bool {
        if self.extended_key {
            self.extended_key = false;
            return false;
        }
        let first = std::mem::replace(&mut self.first, false);
        match byte {
            0 => self.extended_key = true,
            b'\r' => {
                self.bytes.push(byte);
                self.echo(output, b"\r");
                return true;
            }
            // Discard a leading LF left over from redirected CRLF input.
            b'\n' => {
                if !first {
                    self.echo(output, b"\r\n");
                }
            }
            8 | 127 => {
                if let Some(byte) = self.bytes.pop() {
                    self.echo(output, b"\x08 \x08");
                    if byte < b' ' && byte != b'\t' {
                        self.echo(output, b"\x08 \x08");
                    }
                }
            }
            _ if self.bytes.len() == usize::from(self.capacity) - 1 => {
                self.echo(output, b"\x07");
            }
            _ => {
                self.bytes.push(byte);
                if byte < b' ' && byte != b'\t' {
                    self.echo(output, &[b'^', byte + b'@']);
                } else {
                    self.echo(output, &[byte]);
                }
            }
        }
        false
    }

    fn echo(&self, output: &CharacterOutput<'_>, bytes: &[u8]) {
        for &byte in bytes {
            output.write_cooked(byte);
        }
    }
}
