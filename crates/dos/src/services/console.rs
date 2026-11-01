use super::buffers::write_wrapping;
use crate::{
    interrupt::{return_result, DosContext},
    ServiceError,
};
use relocade_machine::real_mode_interrupt::set_zero;
use relocade_x86::Segment;
use std::future::Future;

mod input;
pub(super) use input::{available as input_available, read_line};

/// AH=0Ch: flush stdin, then complete AL's input service in the current interrupt frame.
pub(super) async fn flush_input(call: &mut DosContext<'_>) {
    // A closed stdin skips the flush but still performs AL's requested service.
    let input = call.process.files().handle(0).cloned();
    let flush = async {
        if let Ok(handle) = input {
            let _ = input::flush(&handle).await;
        }
    };
    // Construct the follow-up before flushing so a pending flush cannot reroute
    // this request through newly replaced standard handles.
    match call.cpu.al() {
        super::READ_CHARACTER_ECHO | super::READ_CHARACTER | super::READ_CHARACTER_CHECK_BREAK => {
            let echo = call.cpu.al() == super::READ_CHARACTER_ECHO;
            let read = read_character(call, echo);
            flush.await;
            read.await;
        }
        super::DIRECT_CONSOLE_IO => {
            let read = direct_io(call);
            flush.await;
            read.await;
        }
        super::READ_BUFFERED => {
            let read = read_buffered(call);
            flush.await;
            read.await;
        }
        _ => {
            flush.await;
            call.cpu.set_al(0);
        }
    }
}

/// AH=0Bh: report stdin availability without consuming input or changing flags.
pub(super) async fn input_status(call: &mut DosContext<'_>) {
    // Closed stdin falls back to the console; AH=0Bh has no handle-error return.
    let input = call.process.files().handle(0).cloned();
    let available = match input {
        Ok(handle) => input::available(&handle).await,
        Err(_) => call.dos.devices().console().input_available().await,
    }
    .unwrap_or(false);
    call.cpu.set_al(if available { 0xff } else { 0 });
}

/// AH=06h: DL=FFh polls stdin without echo; other values write a byte to stdout.
pub(super) fn direct_io<'a, 'b>(
    call: &'a mut DosContext<'b>,
) -> impl Future<Output = ()> + use<'a, 'b> {
    let byte = call.cpu.dl();
    let input = call.process.files().readable_handle(0);
    let output = call.stdout();
    async move {
        if byte == 0xff {
            let result = match input {
                Ok(handle) => input::try_read_character(&handle).await,
                Err(error) => Err(error),
            };
            let character = match result {
                Ok(character) => character,
                Err(error) => return return_result(call, Err(error)),
            };
            call.cpu.set_al(character.unwrap_or(0));
            set_zero(call.cpu, &mut call.memory, character.is_none());
        } else {
            // Like AH=09h, character output has no write-error return.
            let _ = output.write(&[byte]).await;
            call.cpu.set_al(byte);
        }
    }
}

/// AH=01h/07h/08h: wait for one character with the requested echo behavior.
pub(super) fn read_character<'a, 'b>(
    call: &'a mut DosContext<'b>,
    echo: bool,
) -> impl Future<Output = ()> + use<'a, 'b> {
    let input = call.process.files().readable_handle(0);
    let output = echo.then(|| call.stdout());
    async move {
        let handle = match input {
            Ok(input) => input,
            Err(error) => return return_result(call, Err(error)),
        };
        let byte = match input::read_character(&handle).await {
            Ok(byte) => byte,
            Err(error) => return return_result(call, Err(error)),
        };
        if let Some(output) = output {
            let _ = output.write_cooked(byte).await;
        }
        call.cpu.set_al(byte);
    }
}

/// AH=02h: write DL through stdout; AL is the last output byte.
pub(super) async fn write_character(call: &mut DosContext<'_>) {
    let byte = call.cpu.dl();
    let _ = call.stdout().write_cooked(byte).await;
    let last = if byte == b'\t' { b' ' } else { byte };
    call.cpu.set_al(last);
}

/// AH=0Ah: edit a line into DS:DX's buffer, whose capacity includes the final CR.
pub(super) fn read_buffered<'a, 'b>(
    call: &'a mut DosContext<'b>,
) -> impl Future<Output = ()> + use<'a, 'b> {
    let segment = call.cpu.segment(Segment::Ds);
    let offset = call.cpu.dx();
    let mut capacity = [0];
    call.memory
        .read_segment(&segment, u32::from(offset), &mut capacity)
        .expect("real-mode line buffer");
    let input = call.process.files().readable_handle(0);
    let output = call.stdout();
    async move {
        if capacity[0] == 0 {
            return;
        }
        let input = match input {
            Ok(input) => input,
            Err(error) => return return_result(call, Err(error)),
        };
        let bytes = match read_line(&input, capacity[0], output).await {
            Ok(bytes) => bytes,
            Err(error) => return return_result(call, Err(error)),
        };
        let count = (bytes.len() - 1) as u8; // DOS's count excludes the carriage return.
        write_wrapping(&mut call.memory, &segment, offset.wrapping_add(1), &[count]);
        write_wrapping(&mut call.memory, &segment, offset.wrapping_add(2), &bytes);
    }
}

/// AH=09h: output the '$'-terminated string at DS:DX and return AL='$'.
pub(super) async fn write_string(call: &mut DosContext<'_>) {
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
        let _ = output.write_cooked(byte[0]).await;
    }
    call.exit(Err(ServiceError::UnterminatedString))
}
