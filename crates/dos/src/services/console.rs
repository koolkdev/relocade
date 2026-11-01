use super::buffers::write_wrapping;
use crate::{
    interrupt::{return_result, DosContext},
    ServiceError,
};
use relocade_machine::real_mode_interrupt::set_zero;
use relocade_x86::Segment;

mod input;
pub(super) use input::{available as input_available, read_line};

/// AH=0Ch: flush stdin, then complete AL's input service in the current interrupt frame.
pub(super) async fn flush_input(call: &mut DosContext<'_>) {
    // A closed stdin skips the flush but still performs AL's requested service.
    if let Ok(handle) = call.process.files().handle(0) {
        input::flush(handle, &call.dos.stdio().input);
    }
    match call.cpu.al() {
        super::READ_CHARACTER_ECHO => read_character(call, true).await,
        super::READ_CHARACTER | super::READ_CHARACTER_CHECK_BREAK => {
            read_character(call, false).await
        }
        super::DIRECT_CONSOLE_IO => direct_io(call),
        super::READ_BUFFERED => read_buffered(call).await,
        _ => {
            call.cpu.set_al(0);
        }
    }
}

/// AH=0Bh: report stdin availability without consuming input or changing flags.
pub(super) fn input_status(call: &mut DosContext<'_>) {
    // Closed stdin falls back to the console; AH=0Bh has no handle-error return.
    let available = call
        .process
        .files()
        .handle(0)
        .map(|handle| input::available(handle, &call.dos.stdio().input));
    let available = available.unwrap_or_else(|_| call.dos.stdio().input.has_input());
    call.cpu.set_al(if available { 0xff } else { 0 });
}

/// AH=06h: DL=FFh polls stdin without echo; other values write a byte to stdout.
pub(super) fn direct_io(call: &mut DosContext<'_>) {
    let byte = call.cpu.dl();
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
        call.cpu.set_al(character.unwrap_or(0));
        set_zero(call.cpu, &mut call.memory, character.is_none());
    } else {
        // Like AH=09h, character output has no write-error return.
        let _ = call.stdout().write(&[byte]);
        call.cpu.set_al(byte);
    }
}

/// AH=01h/07h/08h: wait for one character with the requested echo behavior.
pub(super) async fn read_character(call: &mut DosContext<'_>, echo: bool) {
    let handle = match call.process.files().readable_handle(0) {
        Ok(input) => input,
        Err(error) => return return_result(call, Err(error)),
    };
    let output = echo.then(|| call.stdout());
    let byte = match input::read_character(&handle, &call.dos.stdio().input).await {
        Ok(byte) => byte,
        Err(error) => return return_result(call, Err(error)),
    };
    if let Some(output) = output {
        output.write_cooked(byte);
    }
    call.cpu.set_al(byte);
}

/// AH=02h: write DL through stdout; AL is the last output byte.
pub(super) fn write_character(call: &mut DosContext<'_>) {
    let byte = call.cpu.dl();
    let last = call.stdout().write_cooked(byte);
    call.cpu.set_al(last);
}

/// AH=0Ah: edit a line into DS:DX's buffer, whose capacity includes the final CR.
pub(super) async fn read_buffered(call: &mut DosContext<'_>) {
    let segment = call.cpu.segment(Segment::Ds);
    let offset = call.cpu.dx();
    let mut capacity = [0];
    call.memory
        .read_segment(&segment, u32::from(offset), &mut capacity)
        .expect("real-mode line buffer");
    if capacity[0] == 0 {
        return;
    }
    let input = match call.process.files().readable_handle(0) {
        Ok(input) => input,
        Err(error) => return return_result(call, Err(error)),
    };
    let output = call.stdout();
    let bytes = match read_line(&call.dos.stdio().input, &input, capacity[0], output).await {
        Ok(bytes) => bytes,
        Err(error) => return return_result(call, Err(error)),
    };
    let count = (bytes.len() - 1) as u8; // DOS's count excludes the carriage return.
    write_wrapping(&mut call.memory, &segment, offset.wrapping_add(1), &[count]);
    write_wrapping(&mut call.memory, &segment, offset.wrapping_add(2), &bytes);
}

/// AH=09h: output the '$'-terminated string at DS:DX and return AL='$'.
pub(super) fn write_string(call: &mut DosContext<'_>) {
    let segment = call.cpu.segment(Segment::Ds);
    let start = call.cpu.dx();
    let memory = &call.memory;
    let output = call.stdout();
    for index in 0..=u16::MAX {
        let mut byte = [0];
        memory
            .read_segment(&segment, u32::from(start.wrapping_add(index)), &mut byte)
            .expect("real-mode byte address");
        if byte[0] == b'$' {
            call.cpu.set_al(b'$');
            return;
        }
        output.write_cooked(byte[0]);
    }
    call.exit(Err(ServiceError::UnterminatedString))
}
