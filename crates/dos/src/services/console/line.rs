use super::output::{write_cooked_character, CharacterOutput};
use crate::{
    error::DosError,
    files::{Device, HandleType},
    interrupt::{self, return_result},
    state::DosState,
    DosServices,
};
use relocade_machine::{CpuContext, HookResult};

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
        call: &mut CpuContext<'_, DosServices>,
        state: &mut DosState,
        complete: impl FnOnce(&mut CpuContext<'_, DosServices>, &mut DosState, Vec<u8>) -> HookResult
            + 'static,
    ) -> HookResult {
        match self.read_available(call, state) {
            Ok(true) => return complete(call, state, self.bytes),
            Ok(false) => {}
            Err(error) => return return_result(call, Err(error)),
        }
        let input = state
            .process
            .as_ref()
            .expect("active DOS process")
            .resources()
            .files
            .wait_for_character(&state.stdio.input, self.input_handle);
        let input = match input {
            Ok(input) => input,
            Err(error) => return return_result(call, Err(error)),
        };
        interrupt::defer(call, input, move |call, state, byte| {
            if self.accept(call, state, byte) {
                complete(call, state, self.bytes)
            } else {
                self.read(call, state, complete)
            }
        })
    }

    /// Consumes queued input, returning true when Enter completes the line.
    fn read_available(
        &mut self,
        call: &mut CpuContext<'_, DosServices>,
        state: &mut DosState,
    ) -> Result<bool, DosError> {
        // NUL returns zero forever, so it cannot complete a line. Wait without spinning.
        let process = state.process.as_ref().expect("active DOS process").clone();
        let kind = process
            .resources()
            .files
            .readable_handle_type(self.input_handle)?;
        if matches!(kind, HandleType::Device(Device::Null)) {
            return Ok(false);
        }
        loop {
            let next = process
                .resources_mut()
                .files
                .read_character(&state.stdio.input, self.input_handle)?;
            let Some(byte) = next else {
                return Ok(false);
            };
            if self.accept(call, state, byte) {
                return Ok(true);
            }
        }
    }

    fn accept(
        &mut self,
        call: &mut CpuContext<'_, DosServices>,
        state: &mut DosState,
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
                self.echo(call, state, b"\r");
                return true;
            }
            // Discard a leading LF left over from redirected CRLF input.
            b'\n' => {
                if !first {
                    self.echo(call, state, b"\r\n");
                }
            }
            8 | 127 => {
                if let Some(byte) = self.bytes.pop() {
                    self.echo(call, state, b"\x08 \x08");
                    if byte < b' ' && byte != b'\t' {
                        self.echo(call, state, b"\x08 \x08");
                    }
                }
            }
            _ if self.bytes.len() == usize::from(self.capacity) - 1 => {
                self.echo(call, state, b"\x07");
            }
            _ => {
                self.bytes.push(byte);
                if byte < b' ' && byte != b'\t' {
                    self.echo(call, state, &[b'^', byte + b'@']);
                } else {
                    self.echo(call, state, &[byte]);
                }
            }
        }
        false
    }

    fn echo(&self, call: &mut CpuContext<'_, DosServices>, state: &mut DosState, bytes: &[u8]) {
        for &byte in bytes {
            write_cooked_character(call, state, self.echo, byte);
        }
    }
}
