//! DOS handle transfers retain arguments and buffers across ordinary I/O waits.

use crate::{
    console::{CharacterOutput, LineEditor},
    devices::CONTROL_C,
    files::Handle,
    services::{console::Input, control::Request},
    DosError,
};
use relocade_machine::{real_mode_interrupt::interrupt, CpuContext, FarPointer};

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const LINE_CAPACITY: u8 = 128;

/// AH=3Fh/40h stop at the segment end instead of wrapping a file transfer buffer.
fn transfer_count(buffer: FarPointer, count: u16) -> usize {
    usize::from(count).min(0x10000 - usize::from(buffer.offset))
}

#[interrupt(context(call), args(handle = bx, buffer = ptr(ds, dx), count = cx), returns {
    Ok(count) => { ax = count, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(in crate::services) async fn read(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    handle: u16,
    buffer: FarPointer,
    count: u16,
) -> Result<u16, DosError> {
    let count = transfer_count(buffer, count);
    let services = request.services;
    let stdio = services.dos.stdio();
    'operation: loop {
        let source = services.process.files().readable_handle(handle)?;
        let bytes = match &source {
            Handle::Device(handle) if handle.device.is_null() => {
                if count != 0 {
                    handle.set_eof(true);
                }
                Vec::new()
            }
            Handle::Device(handle) if handle.eof() || count == 0 => Vec::new(),
            Handle::Device(handle) if handle.device.is_console() && !handle.raw() => {
                if !stdio.has_buffered_line() {
                    let input = Input::console(services, source.clone());
                    let mut editor = LineEditor::new(LINE_CAPACITY, async |byte| {
                        input.echo(byte).await;
                        Ok::<_, DosError>(())
                    });
                    loop {
                        let byte = input.character(call, request, true).await?;
                        if byte == CONTROL_C {
                            request.handle_break(call).await;
                            continue 'operation;
                        }
                        if editor.accept(byte).await? {
                            break;
                        }
                    }
                    let mut bytes = editor.finish();
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
        call.memory
            .write_wrapping(buffer, &bytes)
            .expect("real-mode read buffer");
        return Ok(bytes.len() as u16);
    }
}

/// A zero-length AH=40h write truncates or extends a file at its current cursor.
#[interrupt(context(call), args(handle = bx, buffer = ptr(ds, dx), count = cx), returns {
    Ok(count) => { ax = count, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(in crate::services) async fn write(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    handle: u16,
    buffer: FarPointer,
    count: u16,
) -> Result<u16, DosError> {
    let count = transfer_count(buffer, count);
    let services = request.services;
    'operation: loop {
        // INT23 may change guest memory or redirection before restarting the call.
        let mut bytes = vec![0; count];
        call.memory
            .read_wrapping(buffer, &mut bytes)
            .expect("real-mode write buffer");
        let destination = services.process.files().writable_handle(handle)?;
        let count = match &destination {
            Handle::Device(handle) if handle.device.is_console() && !handle.raw() => {
                handle.set_eof(false);
                // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
                let count = bytes
                    .iter()
                    .position(|&byte| byte == 0x1a)
                    .unwrap_or(bytes.len());
                let output = CharacterOutput::handle(services.dos.stdio(), Ok(destination.clone()));
                let input = Input::new(services);
                for &byte in &bytes[..count] {
                    if input.break_pending().await {
                        request.handle_break(call).await;
                        continue 'operation;
                    }
                    output.write_cooked(byte).await?;
                }
                count
            }
            _ => {
                let count = destination.write(&bytes).await?;
                if let Handle::Device(handle) = destination {
                    handle.set_eof(false);
                }
                count
            }
        };
        return Ok(count as u16);
    }
}
