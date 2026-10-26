//! Console handle reads, including cooked lines and raw byte transfers.

use super::{
    line::LineInput,
    output::{write_console, CharacterOutput},
};
use crate::{
    interrupt::{self, return_result},
    services::buffers::transfer_count,
    state::DosState,
    stdio::OutputStream,
    DosServices,
};
use relocade_machine::{CpuContext, HookResult};
use relocade_x86::Segment;

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

/// AH=3Fh: read an edited line or the requested raw byte count, according to the handle.
pub(in crate::services) fn read_handle(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
    stream: OutputStream,
) -> HookResult {
    let handle = call.cpu.bx();
    if state.files.console_eof(handle) {
        call.cpu.set_ax(0);
        return return_result(call, Ok(()));
    }
    if state.files.console_raw(handle) {
        return read_raw(call, state);
    }
    if call.cpu.cx() == 0 || !state.files.stdio.console_line.is_empty() {
        return return_handle_buffer(call, state);
    }
    let line = LineInput::new(
        CONSOLE_LINE_CAPACITY,
        handle,
        CharacterOutput::Console(stream),
    );
    line.read(call, state, move |call, state, bytes| {
        return_handle_line(call, state, bytes, stream)
    })
}

fn return_handle_line(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
    mut bytes: Vec<u8>,
    stream: OutputStream,
) -> HookResult {
    let handle = call.cpu.bx();
    let eof = bytes.first() == Some(&0x1a);
    state.files.set_console_eof(handle, eof);
    write_console(call, state, stream, b"\n");
    // Ctrl-Z at the start of a console line reports EOF.
    if !eof {
        bytes.push(b'\n');
        state.files.stdio.console_line.extend(bytes);
    }
    return_handle_buffer(call, state)
}

fn return_handle_buffer(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let cpu = &call.cpu;
    let count = transfer_count(cpu.dx(), cpu.cx());
    let line = &mut state.files.stdio.console_line;
    let count = count.min(line.len());
    let bytes: Vec<_> = line.drain(..count).collect();
    return_bytes(call, &bytes)
}

fn read_raw(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let cpu = &call.cpu;
    let count = transfer_count(cpu.dx(), cpu.cx());
    let input = state.files.stdio.input.clone();
    let mut bytes = Vec::with_capacity(count);
    while bytes.len() < count {
        match input.try_read_character() {
            Some(byte) => bytes.push(byte),
            None => {
                return interrupt::defer(
                    call,
                    async move {
                        // A raw console read waits for CX bytes, including control characters.
                        while bytes.len() < count {
                            bytes.push(input.read_character().await);
                        }
                        bytes
                    },
                    |call, _, bytes| return_bytes(call, &bytes),
                );
            }
        }
    }
    return_bytes(call, &bytes)
}

fn return_bytes(call: &mut CpuContext<'_, DosServices>, bytes: &[u8]) -> HookResult {
    if !bytes.is_empty() {
        let cpu = &call.cpu;
        let offset = cpu.dx();
        let segment = cpu.segment(Segment::Ds);
        call.memory
            .write_segment(&segment, u32::from(offset), bytes)
            .expect("real-mode console read buffer");
    }
    call.cpu.set_ax(bytes.len() as u16);
    return_result(call, Ok(()))
}
