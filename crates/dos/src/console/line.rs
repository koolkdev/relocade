use crate::error::DosError;

pub(crate) async fn edit_line(
    mut read: impl AsyncFnMut() -> Result<u8, DosError>,
    capacity: u8,
    write: impl AsyncFn(u8) -> Result<(), DosError>,
) -> Result<Vec<u8>, DosError> {
    let mut editor = LineEditor {
        bytes: Vec::new(),
        capacity,
        write,
        extended_key: false,
        first: true,
    };
    loop {
        let byte = read().await?;
        if editor.accept(byte).await? {
            return Ok(editor.bytes);
        }
    }
}

/// A line being edited, including its final carriage return once complete.
struct LineEditor<W> {
    bytes: Vec<u8>,
    capacity: u8,
    write: W,
    extended_key: bool,
    first: bool,
}

impl<W: AsyncFn(u8) -> Result<(), DosError>> LineEditor<W> {
    async fn accept(&mut self, byte: u8) -> Result<bool, DosError> {
        if self.extended_key {
            self.extended_key = false;
            return Ok(false);
        }
        let first = std::mem::replace(&mut self.first, false);
        match byte {
            0 => self.extended_key = true,
            b'\r' => {
                self.bytes.push(byte);
                self.echo(b"\r").await?;
                return Ok(true);
            }
            // Discard a leading LF left over from redirected CRLF input.
            b'\n' => {
                if !first {
                    self.echo(b"\r\n").await?;
                }
            }
            8 | 127 => {
                if let Some(byte) = self.bytes.pop() {
                    self.echo(b"\x08 \x08").await?;
                    if byte < b' ' && byte != b'\t' {
                        self.echo(b"\x08 \x08").await?;
                    }
                }
            }
            _ if self.bytes.len() == usize::from(self.capacity) - 1 => {
                self.echo(b"\x07").await?;
            }
            _ => {
                self.bytes.push(byte);
                if byte < b' ' && byte != b'\t' {
                    self.echo(&[b'^', byte + b'@']).await?;
                } else {
                    self.echo(&[byte]).await?;
                }
            }
        }
        Ok(false)
    }

    async fn echo(&self, bytes: &[u8]) -> Result<(), DosError> {
        for &byte in bytes {
            (self.write)(byte).await?;
        }
        Ok(())
    }
}
