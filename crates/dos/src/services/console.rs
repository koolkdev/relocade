use super::buffers::write_wrapping;
use crate::{
    interrupt::{return_result, DosContext},
    ServiceError,
};
use relocade_machine::{
    real_mode_interrupt::{return_from_interrupt, set_zero},
    HookResult,
};
use relocade_x86::Segment;

mod input;
mod line;
mod output;
pub(super) use input::available as input_available;
pub(super) use line::read_line;
use output::write_characters;
pub(super) use output::{write_console, write_cooked_character, CharacterOutput};

/// AH=0Ch: flush stdin, then complete AL's input service in the current interrupt frame.
pub(super) async fn flush_input(call: &mut DosContext) -> HookResult {
    // A closed stdin skips the flush but still performs AL's requested service.
    if let Ok(handle) = call.process.files().handle(0) {
        input::flush(handle, &call.dos.stdio().input);
    }
    match call.execution.cpu.al() {
        super::READ_CHARACTER_ECHO => read_character(call, true).await,
        super::READ_CHARACTER | super::READ_CHARACTER_CHECK_BREAK => {
            read_character(call, false).await
        }
        super::DIRECT_CONSOLE_IO => direct_io(call),
        super::READ_BUFFERED => read_buffered(call).await,
        _ => {
            call.execution.cpu.set_al(0);
            return_from_interrupt(&mut call.execution.cpu)
        }
    }
}

/// AH=0Bh: report stdin availability without consuming input or changing flags.
pub(super) fn input_status(call: &mut DosContext) -> HookResult {
    // Closed stdin falls back to the console; AH=0Bh has no handle-error return.
    let available = call
        .process
        .files()
        .handle(0)
        .map(|handle| input::available(handle, &call.dos.stdio().input));
    let available = available.unwrap_or_else(|_| call.dos.stdio().input.has_input());
    call.execution.cpu.set_al(if available { 0xff } else { 0 });
    return_from_interrupt(&mut call.execution.cpu)
}

/// AH=06h: DL=FFh polls stdin without echo; other values write a byte to stdout.
pub(super) fn direct_io(call: &mut DosContext) -> HookResult {
    let byte = call.execution.cpu.dl();
    if byte == 0xff {
        let result = call
            .process
            .files()
            .readable_handle(0)
            .and_then(|handle| input::try_read_character(&handle, &call.dos.stdio().input));
        let character = match result {
            Ok(character) => character,
            Err(error) => return return_result(call, Err(error)),
        };
        call.execution.cpu.set_al(character.unwrap_or(0));
        set_zero(
            &call.execution.cpu,
            &mut call.execution.memory(),
            character.is_none(),
        );
    } else {
        // Like AH=09h, character output has no write-error return.
        let _ = write_characters(call, 1, &[byte]);
        call.execution.cpu.set_al(byte);
    }
    return_from_interrupt(&mut call.execution.cpu)
}

/// AH=01h/07h/08h: wait for one character with the requested echo behavior.
pub(super) async fn read_character(call: &mut DosContext, echo: bool) -> HookResult {
    let handle = call.process.files().readable_handle(0);
    let handle = match handle {
        Ok(input) => input,
        Err(error) => return return_result(call, Err(error)),
    };
    let byte = match input::read_character(&handle, &call.dos.stdio().input).await {
        Ok(byte) => byte,
        Err(error) => return return_result(call, Err(error)),
    };
    if echo {
        write_cooked_character(call, CharacterOutput::Stdout, byte);
    }
    call.execution.cpu.set_al(byte);
    return_from_interrupt(&mut call.execution.cpu)
}

/// AH=02h: write DL through stdout; AL is the last output byte.
pub(super) fn write_character(call: &mut DosContext) -> HookResult {
    let byte = call.execution.cpu.dl();
    let last = write_cooked_character(call, CharacterOutput::Stdout, byte);
    call.execution.cpu.set_al(last);
    return_from_interrupt(&mut call.execution.cpu)
}

/// AH=0Ah: edit a line into DS:DX's buffer, whose capacity includes the final CR.
pub(super) async fn read_buffered(call: &mut DosContext) -> HookResult {
    let segment = call.execution.cpu.segment(Segment::Ds);
    let offset = call.execution.cpu.dx();
    let mut capacity = [0];
    call.execution
        .memory()
        .read_segment(&segment, u32::from(offset), &mut capacity)
        .expect("real-mode line buffer");
    if capacity[0] == 0 {
        return return_from_interrupt(&mut call.execution.cpu);
    }
    let input = call.process.files().readable_handle(0);
    let input = match input {
        Ok(input) => input,
        Err(error) => return return_result(call, Err(error)),
    };
    let bytes = match read_line(call, &input, capacity[0], CharacterOutput::Stdout).await {
        Ok(bytes) => bytes,
        Err(error) => return return_result(call, Err(error)),
    };
    let count = (bytes.len() - 1) as u8; // DOS's count excludes the carriage return.
    {
        let memory = &mut call.execution.memory();
        write_wrapping(memory, &segment, offset.wrapping_add(1), &[count]);
        write_wrapping(memory, &segment, offset.wrapping_add(2), &bytes);
    }
    return_from_interrupt(&mut call.execution.cpu)
}

/// AH=09h: output the '$'-terminated string at DS:DX and return AL='$'.
pub(super) fn write_string(call: &mut DosContext) -> HookResult {
    let segment = call.execution.cpu.segment(Segment::Ds);
    let start = call.execution.cpu.dx();
    for index in 0..=u16::MAX {
        let mut byte = [0];
        call.execution
            .memory()
            .read_segment(&segment, u32::from(start.wrapping_add(index)), &mut byte)
            .expect("real-mode byte address");
        if byte[0] == b'$' {
            call.execution.cpu.set_al(b'$');
            return return_from_interrupt(&mut call.execution.cpu);
        }
        write_cooked_character(call, CharacterOutput::Stdout, byte[0]);
    }
    call.exit(Err(ServiceError::UnterminatedString))
}
