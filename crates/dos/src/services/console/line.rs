use crate::{error::DosError, files::Files, stdio::OutputStream};

pub(super) enum Echo {
    Stdout,
    Console,
}

/// A line being edited, including its final carriage return once complete.
pub(super) struct LineInput {
    pub(super) bytes: Vec<u8>,
    capacity: u8,
    input_handle: u16,
    echo: Echo,
    extended_key: bool,
    first: bool,
}

impl LineInput {
    /// Capacity includes the carriage return and must be nonzero.
    pub(super) fn new(capacity: u8, input_handle: u16, echo: Echo) -> Self {
        Self {
            bytes: Vec::new(),
            capacity,
            input_handle,
            echo,
            extended_key: false,
            first: true,
        }
    }

    /// Consumes queued input, returning true when Enter completes the line.
    pub(super) fn read_available(&mut self, files: &mut Files) -> Result<bool, DosError> {
        while let Some(byte) = files.read_character(self.input_handle)? {
            if self.accept(files, byte) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) async fn read_remaining(&mut self, files: &mut Files) -> Result<(), DosError> {
        loop {
            let byte = files.wait_for_character(self.input_handle)?.await;
            if self.accept(files, byte) || self.read_available(files)? {
                return Ok(());
            }
        }
    }

    fn accept(&mut self, files: &mut Files, byte: u8) -> bool {
        if self.extended_key {
            self.extended_key = false;
            return false;
        }
        let first = std::mem::replace(&mut self.first, false);
        match byte {
            0 => self.extended_key = true,
            b'\r' => {
                self.bytes.push(byte);
                self.echo(files, b"\r");
                return true;
            }
            // Discard a leading LF left over from redirected CRLF input.
            b'\n' => {
                if !first {
                    self.echo(files, b"\r\n");
                }
            }
            8 | 127 => {
                if let Some(byte) = self.bytes.pop() {
                    self.echo(files, b"\x08 \x08");
                    if byte < b' ' && byte != b'\t' {
                        self.echo(files, b"\x08 \x08");
                    }
                }
            }
            _ if self.bytes.len() == usize::from(self.capacity) - 1 => {
                self.echo(files, b"\x07");
            }
            _ => {
                self.bytes.push(byte);
                if byte < b' ' && byte != b'\t' {
                    self.echo(files, &[b'^', byte + b'@']);
                } else {
                    self.echo(files, &[byte]);
                }
            }
        }
        false
    }

    fn echo(&self, files: &mut Files, bytes: &[u8]) {
        match self.echo {
            Echo::Stdout => {
                // DOS character input has no echo-write error return.
                let _ = files.write(1, bytes);
            }
            // Handle reads echo to the console device, regardless of stdout redirection.
            Echo::Console => files.stdio.output(OutputStream::Stdout).extend(bytes),
        }
    }
}
