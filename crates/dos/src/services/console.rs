use super::{buffers::write_wrapping, return_to_host};
use crate::{
    interrupt::{self, return_result},
    state::DosState,
    DosServices, ServiceError,
};
use relocade_machine::{
    real_mode_interrupt::{return_from_interrupt, set_zero},
    CpuContext, HookResult,
};
use relocade_x86::{Segment, SegmentRegister};

mod handles;
mod line;
mod output;
pub(super) use handles::read_handle;
use line::LineInput;
pub(super) use output::write_handle;
use output::{write_characters, write_cooked_character, CharacterOutput};

/// AH=0Ch: flush stdin, then invoke the input service selected by AL.
pub(super) fn flush_input(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    // DOS proceeds to the requested service even if the flush failed.
    let _ = state.files.flush_input(0);
    let function = call.cpu.al();
    if matches!(
        function,
        super::READ_CHARACTER_ECHO
            | super::DIRECT_CONSOLE_IO
            | super::READ_CHARACTER
            | super::READ_CHARACTER_CHECK_BREAK
            | super::READ_BUFFERED
    ) {
        call.cpu.set_ah(function);
        // Reuse the current interrupt frame, including when this read defers.
        super::int21(call, state)
    } else {
        call.cpu.set_al(0);
        return_from_interrupt(call)
    }
}

/// AH=0Bh: report stdin availability without consuming input or changing flags.
pub(super) fn input_status(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    // Closed stdin falls back to the console; AH=0Bh has no handle-error return.
    let available = state
        .files
        .input_available(0)
        .unwrap_or_else(|_| state.files.stdio.input.has_input());
    call.cpu.set_al(if available { 0xff } else { 0 });
    return_from_interrupt(call)
}

/// AH=06h: DL=FFh polls stdin without echo; other values write a byte to stdout.
pub(super) fn direct_io(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let byte = call.cpu.dl();
    if byte == 0xff {
        let result = state.files.read_character(0);
        let character = match result {
            Ok(character) => character,
            Err(error) => return return_result(call, Err(error)),
        };
        call.cpu.set_al(character.unwrap_or(0));
        set_zero(call, character.is_none());
    } else {
        // Like AH=09h, character output has no write-error return.
        let _ = write_characters(call, state, 1, &[byte]);
        call.cpu.set_al(byte);
    }
    return_from_interrupt(call)
}

/// AH=01h/07h/08h: wait for one character, echoing through stdout for AH=01h.
pub(super) fn read_character(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let echo = call.cpu.ah() == super::READ_CHARACTER_ECHO;
    match state.files.read_character(0) {
        Ok(Some(byte)) => return_character(call, state, byte, echo),
        Ok(None) => match state.files.wait_for_character(0) {
            Ok(input) => interrupt::defer(call, input, move |call, state, byte| {
                return_character(call, state, byte, echo)
            }),
            Err(error) => return_result(call, Err(error)),
        },
        Err(error) => return_result(call, Err(error)),
    }
}

fn return_character(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
    byte: u8,
    echo: bool,
) -> HookResult {
    if echo {
        write_cooked_character(call, state, CharacterOutput::Stdout, byte);
    }
    call.cpu.set_al(byte);
    return_from_interrupt(call)
}

/// AH=02h: write DL through stdout; AL is the last output byte.
pub(super) fn write_character(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let byte = call.cpu.dl();
    let last = write_cooked_character(call, state, CharacterOutput::Stdout, byte);
    call.cpu.set_al(last);
    return_from_interrupt(call)
}

/// AH=0Ah: edit a line into DS:DX's buffer, whose capacity includes the final CR.
pub(super) fn read_buffered(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let segment = call.cpu.segment(Segment::Ds);
    let offset = call.cpu.dx();
    let mut capacity = [0];
    call.memory
        .read_segment(&segment, u32::from(offset), &mut capacity)
        .expect("real-mode line buffer");
    if capacity[0] == 0 {
        return return_from_interrupt(call);
    }
    let line = LineInput::new(capacity[0], 0, CharacterOutput::Stdout);
    line.read(call, state, move |call, _, bytes| {
        return_line(call, &segment, offset, &bytes)
    })
}

fn return_line(
    call: &mut CpuContext<'_, DosServices>,
    segment: &SegmentRegister,
    offset: u16,
    bytes: &[u8],
) -> HookResult {
    let count = (bytes.len() - 1) as u8; // DOS's count excludes the carriage return.
    let memory = &mut call.memory;
    write_wrapping(memory, segment, offset.wrapping_add(1), &[count]);
    write_wrapping(memory, segment, offset.wrapping_add(2), bytes);
    return_from_interrupt(call)
}

/// AH=09h: output the '$'-terminated string at DS:DX and return AL='$'.
pub(super) fn write_string(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let segment = call.cpu.segment(Segment::Ds);
    let start = call.cpu.dx();
    for index in 0..=u16::MAX {
        let mut byte = [0];
        call.memory
            .read_segment(&segment, u32::from(start.wrapping_add(index)), &mut byte)
            .expect("real-mode byte address");
        if byte[0] == b'$' {
            call.cpu.set_al(b'$');
            return return_from_interrupt(call);
        }
        write_cooked_character(call, state, CharacterOutput::Stdout, byte[0]);
    }
    return_to_host(call, state, Err(ServiceError::UnterminatedString))
}
