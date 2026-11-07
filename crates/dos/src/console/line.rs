pub(crate) async fn edit_line<E>(
    mut read: impl AsyncFnMut() -> Result<u8, E>,
    capacity: u8,
    write: impl AsyncFn(u8) -> Result<(), E>,
) -> Result<Vec<u8>, E> {
    let mut editor = LineEditor::new(capacity, write);
    loop {
        let byte = read().await?;
        if editor.accept(byte).await? {
            return Ok(editor.finish());
        }
    }
}

/// A line being edited, including its final carriage return once complete.
pub(crate) struct LineEditor<W> {
    bytes: Vec<u8>,
    capacity: u8,
    write: W,
    extended_key: bool,
    first: bool,
}

impl<W> LineEditor<W> {
    pub(crate) fn new(capacity: u8, write: W) -> Self {
        Self {
            bytes: Vec::new(),
            capacity,
            write,
            extended_key: false,
            first: true,
        }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) async fn accept<E>(&mut self, byte: u8) -> Result<bool, E>
    where
        W: AsyncFn(u8) -> Result<(), E>,
    {
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

    async fn echo<E>(&self, bytes: &[u8]) -> Result<(), E>
    where
        W: AsyncFn(u8) -> Result<(), E>,
    {
        for &byte in bytes {
            (self.write)(byte).await?;
        }
        Ok(())
    }
}
