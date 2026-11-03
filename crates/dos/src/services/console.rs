use super::Services;
use crate::{DosError, ServiceError};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_match},
    CpuContext, FarPointer, MemoryView,
};

pub(super) mod cooked;
mod input;
mod request;
use request::capacity;
pub(super) use request::Input;

/// AH=0Ch flushes stdin, then invokes the input service selected by AL.
#[interrupt(context(call), args(function = al))]
pub(super) async fn flush_input(call: &mut CpuContext<'_>, services: &Services<'_>, function: u8) {
    let stdin = services.process.files().handle(0).cloned();
    let input = Input::new(services);
    // A closed stdin or failed flush must not skip the selected operation.
    if let Ok(stdin) = stdin {
        let _ = stdin.flush_input().await;
    }
    interrupt_match!(
        call,
        match function {
            0x01 => read_character_echo(input),
            0x06 => direct_io(input),
            0x07 => read_character(input),
            0x08 => read_character_check_break(input),
            0x0a => read_buffered(input),
            _ => no_input,
        }
    );
}

/// Other AL values request only a flush.
#[interrupt(returns { () => { al = 0u8 } })]
fn no_input() {}

/// AH=0Bh reports stdin availability without consuming input or changing flags.
#[interrupt(returns { status => { al = status } })]
pub(super) async fn input_status(services: &Services<'_>) -> u8 {
    let input = services.process.files().handle(0).cloned();
    let available = match input {
        Ok(handle) => handle.input_available().await,
        Err(_) => services.dos.devices().console().input_available().await,
    }
    .unwrap_or(false);
    if available {
        0xff
    } else {
        0
    }
}

// AH=06h polls stdin for DL=FFh; other values write a byte to stdout.
#[interrupt(context(call), args(byte = dl))]
pub(super) async fn direct_io(call: &mut CpuContext<'_>, input: Input<'_>, byte: u8) {
    interrupt_match!(
        call,
        match byte {
            0xff => input.poll_character,
            _ => input.write_raw,
        }
    );
}

/// AH=01h waits for a character and echoes it through the retained stdout.
#[interrupt(returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character_echo(input: Input<'_>) -> Result<u8, DosError> {
    input.character(true).await
}

/// AH=07h waits for a character without echo.
#[interrupt(returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character(input: Input<'_>) -> Result<u8, DosError> {
    input.character(false).await
}

/// AH=08h currently shares AH=07h behavior; DOS break handling is not implemented.
#[interrupt(returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character_check_break(input: Input<'_>) -> Result<u8, DosError> {
    read_character(input).await
}

/// AH=02h writes through stdout and returns the last output byte.
#[interrupt(args(byte = dl), returns { last => { al = last } })]
pub(super) async fn write_character(services: &Services<'_>, byte: u8) -> u8 {
    let _ = services.stdout().write_cooked(byte).await;
    if byte == b'\t' {
        b' '
    } else {
        byte
    }
}

/// AH=0Ah edits a line; buffer capacity includes the final carriage return.
#[interrupt(context(memory), args(buffer = ptr(ds, dx)), returns {
    Ok(()) => {},
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_buffered(
    memory: &mut MemoryView<'_>,
    input: Input<'_>,
    buffer: FarPointer,
) -> Result<(), DosError> {
    let capacity = capacity(memory, buffer);
    input.buffered(memory, buffer, capacity).await
}

/// AH=09h outputs a '$'-terminated string and returns AL='$'.
#[interrupt(context(memory), args(source = ptr(ds, dx)), returns {
    Ok(byte) => { al = byte },
    Err(error) => super::fatal(services, error),
})]
pub(super) async fn write_string(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
    source: FarPointer,
) -> Result<u8, ServiceError> {
    let output = services.stdout();
    for index in 0..=u16::MAX {
        let mut byte = [0];
        memory
            .read_wrapping(source.wrapping_add(usize::from(index)), &mut byte)
            .expect("real-mode byte address");
        if byte[0] == b'$' {
            return Ok(b'$');
        }
        let _ = output.write_cooked(byte[0]).await;
    }
    Err(ServiceError::UnterminatedString)
}
