//! Console handle reads, including cooked lines and raw byte transfers.

use super::{
    line::LineInput,
    output::{write_console, CharacterOutput},
};
use crate::{
    interrupt::return_result, services::buffers::transfer_count, stdio::OutputStream, DosState,
};
use relocade_runtime::HostCall;
use relocade_x86::{ExecutionExit, Segment};
use std::ops::ControlFlow;

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

/// AH=3Fh: read an edited line or the requested raw byte count, according to the handle.
pub(in crate::services) fn read_handle(
    call: &mut HostCall<'_, DosState>,
    stream: OutputStream,
) -> ControlFlow<ExecutionExit> {
    let handle = call.cpu().bx();
    if call.state().files.console_eof(handle) {
        call.cpu().set_ax(0);
        return return_result(call, Ok(()));
    }
    if call.state().files.console_raw(handle) {
        return read_raw(call);
    }
    if call.cpu().cx() == 0 || !call.state().files.stdio.console_line.is_empty() {
        return return_handle_buffer(call);
    }
    let mut line = LineInput::new(
        CONSOLE_LINE_CAPACITY,
        handle,
        CharacterOutput::Console(stream),
    );
    match line.read_available(call) {
        Ok(true) => return_handle_line(call, line.bytes, stream),
        Ok(false) => call.defer(async move |call| match line.read_remaining(call).await {
            Ok(()) => return_handle_line(call, line.bytes, stream),
            Err(error) => return_result(call, Err(error)),
        }),
        Err(error) => return_result(call, Err(error)),
    }
}

fn return_handle_line(
    call: &mut HostCall<'_, DosState>,
    mut bytes: Vec<u8>,
    stream: OutputStream,
) -> ControlFlow<ExecutionExit> {
    let handle = call.cpu().bx();
    let eof = bytes.first() == Some(&0x1a);
    call.state().files.set_console_eof(handle, eof);
    write_console(call, stream, b"\n");
    // Ctrl-Z at the start of a console line reports EOF.
    if !eof {
        bytes.push(b'\n');
        call.state_mut().files.stdio.console_line.extend(bytes);
    }
    return_handle_buffer(call)
}

fn return_handle_buffer(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let cpu = call.cpu();
    let count = transfer_count(cpu.dx(), cpu.cx());
    let line = &mut call.state_mut().files.stdio.console_line;
    let count = count.min(line.len());
    let bytes: Vec<_> = line.drain(..count).collect();
    return_bytes(call, &bytes)
}

fn read_raw(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let cpu = call.cpu();
    let count = transfer_count(cpu.dx(), cpu.cx());
    let input = call.state().files.stdio.input.clone();
    let mut bytes = Vec::with_capacity(count);
    while bytes.len() < count {
        match input.try_read_character() {
            Some(byte) => bytes.push(byte),
            None => {
                return call.defer(async move |call| {
                    // A raw console read waits for CX bytes, including control characters.
                    while bytes.len() < count {
                        bytes.push(input.read_character().await);
                    }
                    return_bytes(call, &bytes)
                });
            }
        }
    }
    return_bytes(call, &bytes)
}

fn return_bytes(call: &mut HostCall<'_, DosState>, bytes: &[u8]) -> ControlFlow<ExecutionExit> {
    if !bytes.is_empty() {
        let cpu = call.cpu();
        let offset = cpu.dx();
        let segment = cpu.segment(Segment::Ds);
        call.memory()
            .write_segment(&segment, u32::from(offset), bytes)
            .expect("real-mode console read buffer");
    }
    call.cpu().set_ax(bytes.len() as u16);
    return_result(call, Ok(()))
}
