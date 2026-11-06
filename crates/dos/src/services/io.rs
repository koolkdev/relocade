//! DOS I/O policies over retained handles: editing, cooked output and device EOF.
//!
//! Each transfer is one attempt. Break means INT23 has returned and the service
//! must resolve its handle and guest buffer again. Ordinary waits retain them.

use super::control::Request;
use crate::{console::CharacterOutput, files::Handle, DosError};
use relocade_machine::CpuContext;
use std::ops::ControlFlow;

mod input;
mod wait;
pub(super) use input::{check_console_break, read_checked_character, Input};

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const LINE_CAPACITY: u8 = 128;

pub(super) async fn read_handle(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    source: &Handle,
    count: usize,
) -> Result<ControlFlow<(), Vec<u8>>, DosError> {
    let stdio = request.services.dos.stdio();
    let bytes = match source {
        Handle::Device(handle) if handle.device.is_null() => {
            if count != 0 {
                handle.set_eof(true);
            }
            Vec::new()
        }
        Handle::Device(handle) if handle.eof() || count == 0 => Vec::new(),
        Handle::Device(handle) if handle.device.is_console() && !handle.raw() => {
            if !stdio.has_buffered_line() {
                let input = Input::console(request.services, source.clone());
                let ControlFlow::Continue(mut bytes) =
                    input.read_line(call, request, LINE_CAPACITY).await?
                else {
                    return Ok(ControlFlow::Break(()));
                };
                let eof = bytes.first() == Some(&0x1a);
                handle.set_eof(eof);
                let output = CharacterOutput::handle(stdio, Ok(source.clone()));
                output.write_all(b"\n").await?;
                if !eof {
                    bytes.push(b'\n');
                    stdio.buffer_line(bytes);
                }
            }
            stdio.take_line_bytes(count)
        }
        _ => {
            let mut bytes = vec![0; count];
            let count = source.read(&mut bytes).await?;
            bytes.truncate(count);
            bytes
        }
    };
    Ok(ControlFlow::Continue(bytes))
}

pub(super) async fn write_handle(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    destination: &Handle,
    bytes: &[u8],
) -> Result<ControlFlow<(), usize>, DosError> {
    let count = match destination {
        Handle::Device(handle) if handle.device.is_console() && !handle.raw() => {
            handle.set_eof(false);
            // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
            let count = bytes
                .iter()
                .position(|&byte| byte == 0x1a)
                .unwrap_or(bytes.len());
            let output =
                CharacterOutput::handle(request.services.dos.stdio(), Ok(destination.clone()));
            let input = Input::new(request.services);
            for &byte in &bytes[..count] {
                if input.break_pending().await {
                    request.handle_break(call).await;
                    return Ok(ControlFlow::Break(()));
                }
                output.write_cooked(byte).await?;
            }
            count
        }
        _ => {
            let count = destination.write(bytes).await?;
            if let Handle::Device(handle) = destination {
                handle.set_eof(false);
            }
            count
        }
    };
    Ok(ControlFlow::Continue(count))
}
