use super::Services;
use crate::{DosError, ServiceError};
use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};

pub(super) mod cooked;
mod input;
mod request;
use request::{capacity, Input, InputResult};

/// AH=0Ch captures the requested input operation before flushing stdin.
#[interrupt(context(memory), args(operation = al, byte = dl, buffer = ptr(ds, dx)), returns {
    Ok(InputResult::Character(byte)) => { al = byte },
    Ok(InputResult::Polled(Some(byte))) => { al = byte, zf = false },
    Ok(InputResult::Polled(None)) => { al = 0u8, zf = true },
    Ok(InputResult::Buffered) => {},
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn flush_input(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
    operation: u8,
    byte: u8,
    buffer: FarPointer,
) -> Result<InputResult, DosError> {
    let flush = services.process.files().handle(0).cloned();
    let input = Input::new(services);
    let capacity = if operation == 0x0a {
        capacity(memory, buffer)
    } else {
        0
    };
    // A closed stdin skips the flush but still performs the requested operation.
    if let Ok(handle) = flush {
        let _ = handle.flush_input().await;
    }
    match operation {
        0x01 => input.character(true).await.map(InputResult::Character),
        0x07 => input.character(false).await.map(InputResult::Character),
        0x08 => input.character(false).await.map(InputResult::Character),
        0x06 => input.direct(byte).await,
        0x0a => {
            input.buffered(memory, buffer, capacity).await?;
            Ok(InputResult::Buffered)
        }
        _ => Ok(InputResult::Character(0)),
    }
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
    Ok(InputResult::Character(byte)) => { al = byte },
    Ok(InputResult::Polled(Some(byte))) => { al = byte, zf = false },
    Ok(InputResult::Polled(None)) => { al = 0u8, zf = true },
    Ok(InputResult::Buffered) => {},
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn direct_io(services: &Services<'_>, byte: u8) -> Result<InputResult, DosError> {
    Input::new(services).direct(byte).await
}

/// AH=01h waits for a character and echoes it through the retained stdout.
#[interrupt(returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character_echo(services: &Services<'_>) -> Result<u8, DosError> {
    Input::new(services).character(true).await
}

/// AH=07h waits for a character without echo.
#[interrupt(returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character(services: &Services<'_>) -> Result<u8, DosError> {
    Input::new(services).character(false).await
}

/// AH=08h currently shares AH=07h behavior; DOS break handling is not implemented.
#[interrupt(returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character_check_break(services: &Services<'_>) -> Result<u8, DosError> {
    read_character(services).await
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
    services: &Services<'_>,
    buffer: FarPointer,
) -> Result<(), DosError> {
    let capacity = capacity(memory, buffer);
    Input::new(services)
        .buffered(memory, buffer, capacity)
        .await
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
