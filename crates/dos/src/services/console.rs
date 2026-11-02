use super::Services;
use crate::{DosError, ServiceError};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    FarPointer, MemoryView,
};

pub(super) mod cooked;
mod input;
mod request;
pub(super) use request::Input;
use request::{capacity, DirectIo};

// AH=0Ch selects the same input services through AL, with a flush before I/O.
// Each service captures its arguments before that flush can suspend.
interrupt_dispatch! {
    pub(super) flush_input(al, services: &Services<'_>) {
        0x01: read_character_echo(Input::flush_first(services)),
        0x06: direct_io(Input::flush_first(services)),
        0x07: read_character(Input::flush_first(services)),
        0x08: read_character_check_break(Input::flush_first(services)),
        0x0a: read_buffered(Input::flush_first(services)),
        _: flush_only(Input::flush_first(services)),
    }
}

#[interrupt(returns { byte => { al = byte } })]
async fn flush_only(input: Input<'_>) -> u8 {
    input.flush().await;
    0
}

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

/// AH=06h polls stdin for DL=FFh; other values write a byte to stdout.
#[interrupt(args(byte = dl), returns {
    Ok(DirectIo::Output(byte)) => { al = byte },
    Ok(DirectIo::Input(Some(byte))) => { al = byte, zf = false },
    Ok(DirectIo::Input(None)) => { al = 0u8, zf = true },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn direct_io(input: Input<'_>, byte: u8) -> Result<DirectIo, DosError> {
    input.direct(byte).await
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
