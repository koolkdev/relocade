use super::{buffers::write_wrapping, return_to_host};
use crate::{
    interrupt::{return_result, DosContext},
    ServiceError,
};
use relocade_machine::{
    real_mode_interrupt::{return_from_interrupt, set_zero},
    HookResult,
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
pub(super) fn flush_input(call: &mut DosContext<'_, '_>) -> HookResult {
    // DOS proceeds to the requested service even if the flush failed.
    let _ = call.dos.files.flush_input(&call.dos.stdio.input, 0);
    let function = call.execution.cpu.al();
    if matches!(
        function,
        super::READ_CHARACTER_ECHO
            | super::DIRECT_CONSOLE_IO
            | super::READ_CHARACTER
            | super::READ_CHARACTER_CHECK_BREAK
            | super::READ_BUFFERED
    ) {
        call.execution.cpu.set_ah(function);
        // Reuse the current interrupt frame, including when this read defers.
        super::int21(call)
    } else {
        call.execution.cpu.set_al(0);
        return_from_interrupt(call.execution)
    }
}

/// AH=0Bh: report stdin availability without consuming input or changing flags.
pub(super) fn input_status(call: &mut DosContext<'_, '_>) -> HookResult {
    // Closed stdin falls back to the console; AH=0Bh has no handle-error return.
    let available = call
        .dos
        .files
        .input_available(&call.dos.stdio.input, 0)
        .unwrap_or_else(|_| call.dos.stdio.input.has_input());
    call.execution.cpu.set_al(if available { 0xff } else { 0 });
    return_from_interrupt(call.execution)
}

/// AH=06h: DL=FFh polls stdin without echo; other values write a byte to stdout.
pub(super) fn direct_io(call: &mut DosContext<'_, '_>) -> HookResult {
    let byte = call.execution.cpu.dl();
    if byte == 0xff {
        let result = call.dos.files.read_character(&call.dos.stdio.input, 0);
        let character = match result {
            Ok(character) => character,
            Err(error) => return return_result(call, Err(error)),
        };
        call.execution.cpu.set_al(character.unwrap_or(0));
        set_zero(call.execution, character.is_none());
    } else {
        // Like AH=09h, character output has no write-error return.
        let _ = write_characters(call, 1, &[byte]);
        call.execution.cpu.set_al(byte);
    }
    return_from_interrupt(call.execution)
}

/// AH=01h/07h/08h: wait for one character, echoing through stdout for AH=01h.
pub(super) fn read_character(call: &mut DosContext<'_, '_>) -> HookResult {
    let echo = call.execution.cpu.ah() == super::READ_CHARACTER_ECHO;
    match call.dos.files.read_character(&call.dos.stdio.input, 0) {
        Ok(Some(byte)) => return_character(call, byte, echo),
        Ok(None) => match call.dos.files.wait_for_character(&call.dos.stdio.input, 0) {
            Ok(input) => call.defer(input, move |call, byte| return_character(call, byte, echo)),
            Err(error) => return_result(call, Err(error)),
        },
        Err(error) => return_result(call, Err(error)),
    }
}

fn return_character(call: &mut DosContext<'_, '_>, byte: u8, echo: bool) -> HookResult {
    if echo {
        write_cooked_character(call, CharacterOutput::Stdout, byte);
    }
    call.execution.cpu.set_al(byte);
    return_from_interrupt(call.execution)
}

/// AH=02h: write DL through stdout; AL is the last output byte.
pub(super) fn write_character(call: &mut DosContext<'_, '_>) -> HookResult {
    let byte = call.execution.cpu.dl();
    let last = write_cooked_character(call, CharacterOutput::Stdout, byte);
    call.execution.cpu.set_al(last);
    return_from_interrupt(call.execution)
}

/// AH=0Ah: edit a line into DS:DX's buffer, whose capacity includes the final CR.
pub(super) fn read_buffered(call: &mut DosContext<'_, '_>) -> HookResult {
    let segment = call.execution.cpu.segment(Segment::Ds);
    let offset = call.execution.cpu.dx();
    let mut capacity = [0];
    call.execution
        .memory
        .read_segment(&segment, u32::from(offset), &mut capacity)
        .expect("real-mode line buffer");
    if capacity[0] == 0 {
        return return_from_interrupt(call.execution);
    }
    let line = LineInput::new(capacity[0], 0, CharacterOutput::Stdout);
    line.read(call, move |call, bytes| {
        return_line(call, &segment, offset, &bytes)
    })
}

fn return_line(
    call: &mut DosContext<'_, '_>,
    segment: &SegmentRegister,
    offset: u16,
    bytes: &[u8],
) -> HookResult {
    let count = (bytes.len() - 1) as u8; // DOS's count excludes the carriage return.
    let memory = &mut call.execution.memory;
    write_wrapping(memory, segment, offset.wrapping_add(1), &[count]);
    write_wrapping(memory, segment, offset.wrapping_add(2), bytes);
    return_from_interrupt(call.execution)
}

/// AH=09h: output the '$'-terminated string at DS:DX and return AL='$'.
pub(super) fn write_string(call: &mut DosContext<'_, '_>) -> HookResult {
    let segment = call.execution.cpu.segment(Segment::Ds);
    let start = call.execution.cpu.dx();
    for index in 0..=u16::MAX {
        let mut byte = [0];
        call.execution
            .memory
            .read_segment(&segment, u32::from(start.wrapping_add(index)), &mut byte)
            .expect("real-mode byte address");
        if byte[0] == b'$' {
            call.execution.cpu.set_al(b'$');
            return return_from_interrupt(call.execution);
        }
        write_cooked_character(call, CharacterOutput::Stdout, byte[0]);
    }
    return_to_host(call, Err(ServiceError::UnterminatedString))
}
