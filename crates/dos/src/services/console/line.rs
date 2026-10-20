use super::output::{write_cooked_character, CharacterOutput};
use crate::{
    error::DosError,
    files::{Device, HandleType},
    DosState,
};
use relocade_runtime::HostCall;

/// A line being edited, including its final carriage return once complete.
pub(super) struct LineInput {
    pub(super) bytes: Vec<u8>,
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

    /// Consumes queued input, returning true when Enter completes the line.
    pub(super) fn read_available(
        &mut self,
        call: &mut HostCall<'_, DosState>,
    ) -> Result<bool, DosError> {
        // NUL returns zero forever, so it cannot complete a line. Wait without spinning.
        if matches!(
            call.state().files.readable_handle_type(self.input_handle)?,
            HandleType::Device(Device::Null)
        ) {
            return Ok(false);
        }
        while let Some(byte) = call.state_mut().files.read_character(self.input_handle)? {
            if self.accept(call, byte) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) async fn read_remaining(
        &mut self,
        call: &mut HostCall<'_, DosState>,
    ) -> Result<(), DosError> {
        loop {
            let byte = call
                .state()
                .files
                .wait_for_character(self.input_handle)?
                .await;
            if self.accept(call, byte) || self.read_available(call)? {
                return Ok(());
            }
        }
    }

    fn accept(&mut self, call: &mut HostCall<'_, DosState>, byte: u8) -> bool {
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

    fn echo(&self, call: &mut HostCall<'_, DosState>, bytes: &[u8]) {
        for &byte in bytes {
            write_cooked_character(call, self.echo, byte);
        }
    }
}
