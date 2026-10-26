use super::{
    input,
    output::{write_cooked_character, CharacterOutput},
};
use crate::{error::DosError, files::Handle, interrupt::DosContext};

/// Edits a line with capacity for its final carriage return.
pub(in crate::services) async fn read_line(
    call: &mut DosContext<'_, '_>,
    source: &Handle,
    capacity: u8,
    echo: CharacterOutput,
) -> Result<Vec<u8>, DosError> {
    let mut editor = LineEditor {
        bytes: Vec::new(),
        capacity,
        echo,
        extended_key: false,
        first: true,
    };
    loop {
        let byte = input::read_character(source, &call.dos.stdio().input).await?;
        if editor.accept(call, byte) {
            return Ok(editor.bytes);
        }
    }
}

/// A line being edited, including its final carriage return once complete.
struct LineEditor {
    bytes: Vec<u8>,
    capacity: u8,
    echo: CharacterOutput,
    extended_key: bool,
    first: bool,
}

impl LineEditor {
    fn accept(&mut self, call: &mut DosContext<'_, '_>, byte: u8) -> bool {
        if self.extended_key {
            self.extended_key = false;
            return false;
        }
        let first = std::mem::replace(&mut self.first, false);
        match byte {
            0 => self.extended_key = true,
            b'\r' => {
                self.bytes.push(byte);
                self.echo(call, b"\r");
                return true;
            }
            // Discard a leading LF left over from redirected CRLF input.
            b'\n' => {
                if !first {
                    self.echo(call, b"\r\n");
                }
            }
            8 | 127 => {
                if let Some(byte) = self.bytes.pop() {
                    self.echo(call, b"\x08 \x08");
                    if byte < b' ' && byte != b'\t' {
                        self.echo(call, b"\x08 \x08");
                    }
                }
            }
            _ if self.bytes.len() == usize::from(self.capacity) - 1 => {
                self.echo(call, b"\x07");
            }
            _ => {
                self.bytes.push(byte);
                if byte < b' ' && byte != b'\t' {
                    self.echo(call, &[b'^', byte + b'@']);
                } else {
                    self.echo(call, &[byte]);
                }
            }
        }
        false
    }

    fn echo(&self, call: &mut DosContext<'_, '_>, bytes: &[u8]) {
        for &byte in bytes {
            write_cooked_character(call, self.echo, byte);
        }
    }
}
