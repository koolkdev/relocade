use super::output::{write_cooked_character, CharacterOutput};
use crate::{
    error::DosError,
    files::{Device, HandleType},
    interrupt::{return_result, DosContext},
};
use relocade_machine::HookResult;

/// A line being edited, including its final carriage return once complete.
pub(super) struct LineInput {
    bytes: Vec<u8>,
    capacity: u8,
    input_handle: u16,
    echo: CharacterOutput,
    extended_key: bool,
    first: bool,
}

impl LineInput {
    /// Capacity includes the carriage return and must be nonzero.
    pub(super) fn new(capacity: u8, input_handle: u16, echo: CharacterOutput) -> Self {
        Self {
            bytes: Vec::new(),
            capacity,
            input_handle,
            echo,
            extended_key: false,
            first: true,
        }
    }

    /// Reads queued input and defers for missing characters until the line is complete.
    pub(super) fn read(
        mut self,
        call: &mut DosContext<'_, '_>,
        complete: impl FnOnce(&mut DosContext<'_, '_>, Vec<u8>) -> HookResult + 'static,
    ) -> HookResult {
        match self.read_available(call) {
            Ok(true) => return complete(call, self.bytes),
            Ok(false) => {}
            Err(error) => return return_result(call, Err(error)),
        }
        let input = match call
            .process
            .files()
            .wait_for_character(&call.dos.stdio.input, self.input_handle)
        {
            Ok(input) => input,
            Err(error) => return return_result(call, Err(error)),
        };
        call.defer(input, move |call, byte| {
            if self.accept(call, byte) {
                complete(call, self.bytes)
            } else {
                self.read(call, complete)
            }
        })
    }

    /// Consumes queued input, returning true when Enter completes the line.
    fn read_available(&mut self, call: &mut DosContext<'_, '_>) -> Result<bool, DosError> {
        // NUL returns zero forever, so it cannot complete a line. Wait without spinning.
        if matches!(
            call.process
                .files()
                .readable_handle_type(self.input_handle)?,
            HandleType::Device(Device::Null)
        ) {
            return Ok(false);
        }
        while let Some(byte) = call
            .process
            .files()
            .read_character(&call.dos.stdio.input, self.input_handle)?
        {
            if self.accept(call, byte) {
                return Ok(true);
            }
        }
        Ok(false)
    }

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
