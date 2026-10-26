//! Console handle reads, including cooked lines and raw byte transfers.

use super::{
    line::LineInput,
    output::{write_console, CharacterOutput},
};
use crate::{
    interrupt::{return_result, DosContext},
    services::buffers::transfer_count,
    stdio::OutputStream,
};
use relocade_machine::HookResult;
use relocade_x86::Segment;

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

/// AH=3Fh: read an edited line or the requested raw byte count, according to the handle.
pub(in crate::services) fn read_handle(
    call: &mut DosContext<'_, '_>,
    stream: OutputStream,
) -> HookResult {
    let handle = call.execution.cpu.bx();
    if call.process.files().console_eof(handle) {
        call.execution.cpu.set_ax(0);
        return return_result(call, Ok(()));
    }
    if call.process.files().console_raw(handle) {
        return read_raw(call);
    }
    if call.execution.cpu.cx() == 0 || call.dos.stdio().has_buffered_line() {
        return return_handle_buffer(call);
    }
    let line = LineInput::new(
        CONSOLE_LINE_CAPACITY,
        handle,
        CharacterOutput::Console(stream),
    );
    line.read(call, move |call, bytes| {
        return_handle_line(call, bytes, stream)
    })
}

fn return_handle_line(
    call: &mut DosContext<'_, '_>,
    mut bytes: Vec<u8>,
    stream: OutputStream,
) -> HookResult {
    let handle = call.execution.cpu.bx();
    let eof = bytes.first() == Some(&0x1a);
    call.process.files().set_console_eof(handle, eof);
    write_console(call, stream, b"\n");
    // Ctrl-Z at the start of a console line reports EOF.
    if !eof {
        bytes.push(b'\n');
        call.dos.stdio().buffer_line(bytes);
    }
    return_handle_buffer(call)
}

fn return_handle_buffer(call: &mut DosContext<'_, '_>) -> HookResult {
    let cpu = &call.execution.cpu;
    let count = transfer_count(cpu.dx(), cpu.cx());
    let bytes = call.dos.stdio().take_line_bytes(count);
    return_bytes(call, &bytes)
}

fn read_raw(call: &mut DosContext<'_, '_>) -> HookResult {
    let cpu = &call.execution.cpu;
    let count = transfer_count(cpu.dx(), cpu.cx());
    let input = call.dos.stdio().input.clone();
    let mut bytes = Vec::with_capacity(count);
    while bytes.len() < count {
        match input.try_read_character() {
            Some(byte) => bytes.push(byte),
            None => {
                return call.defer(
                    async move {
                        // A raw console read waits for CX bytes, including control characters.
                        while bytes.len() < count {
                            bytes.push(input.read_character().await);
                        }
                        bytes
                    },
                    |call, bytes| return_bytes(call, &bytes),
                );
            }
        }
    }
    return_bytes(call, &bytes)
}

fn return_bytes(call: &mut DosContext<'_, '_>, bytes: &[u8]) -> HookResult {
    if !bytes.is_empty() {
        let cpu = &call.execution.cpu;
        let offset = cpu.dx();
        let segment = cpu.segment(Segment::Ds);
        call.execution
            .memory
            .write_segment(&segment, u32::from(offset), bytes)
            .expect("real-mode console read buffer");
    }
    call.execution.cpu.set_ax(bytes.len() as u16);
    return_result(call, Ok(()))
}
