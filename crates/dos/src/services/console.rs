//! DOS character services and their register bindings.

use super::{
    control::Request,
    io::{self, Input},
};
use crate::{DosError, ServiceError};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, FarPointer,
};
use std::ops::ControlFlow;

/// AH=01h waits for a checked character and echoes through the retained stdout.
#[interrupt(context(call), returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character_echo(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
) -> Result<u8, DosError> {
    io::read_checked_character(call, request, true).await
}

/// AH=07h reads without echo or break checking.
#[interrupt(context(call), returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character(
    call: &mut CpuContext<'_>,
    request: &Request<'_>,
) -> Result<u8, DosError> {
    Input::start(request)
        .await
        .character(call, request, false)
        .await
}

/// AH=08h reads with break checking and without echo.
#[interrupt(context(call), returns {
    Ok(byte) => { al = byte },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_character_check_break(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
) -> Result<u8, DosError> {
    io::read_checked_character(call, request, false).await
}

/// AH=02h writes through stdout and returns the last output byte.
#[interrupt(context(call), args(byte = dl), returns { last => { al = last } })]
pub(super) async fn write_character(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    byte: u8,
) -> u8 {
    loop {
        let input = Input::new(request.services);
        if input.break_pending().await {
            request.handle_break(call).await;
            continue;
        }
        input.write(call, request, byte).await;
        return if byte == b'\t' { b' ' } else { byte };
    }
}

/// AH=06h polls stdin for DL=FFh; other values write a byte to stdout.
#[interrupt(context(call), args(byte = dl))]
pub(super) async fn direct_io(call: &mut CpuContext<'_>, request: &Request<'_>, byte: u8) {
    let input = Input::start(request).await;
    interrupt_dispatch!(
        call,
        match byte {
            0xff => poll_character(input, request),
            _ => write_raw_character(input, byte),
        }
    );
}

#[interrupt(context(call), returns {
    Ok(Some(byte)) => { al = byte, zf = false },
    Ok(None) => { al = 0u8, zf = true },
    Err(error) => crate::interrupt::return_error(error),
})]
async fn poll_character(
    call: &mut CpuContext<'_>,
    input: Input<'_>,
    request: &Request<'_>,
) -> Result<Option<u8>, DosError> {
    input.poll_character(call, request).await
}

// DL was captured by direct_io before the optional AH=0Ch flush could suspend.
#[interrupt(returns { byte => { al = byte } })]
async fn write_raw_character(input: Input<'_>, byte: u8) -> u8 {
    input.write_raw(byte).await;
    byte
}

/// AH=09h outputs a '$'-terminated string and returns AL='$'.
#[interrupt(context(call), args(source = ptr(ds, dx)), returns {
    Ok(byte) => { al = byte },
    Err(error) => request.services.exit(Err(error)),
})]
pub(super) async fn write_string(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    source: FarPointer,
) -> Result<u8, ServiceError> {
    'operation: loop {
        let input = Input::new(request.services);
        for index in 0..=u16::MAX {
            let mut byte = [0];
            call.memory
                .read_wrapping(source.wrapping_add(usize::from(index)), &mut byte)
                .expect("real-mode byte address");
            if byte[0] == b'$' {
                return Ok(b'$');
            }
            if input.break_pending().await {
                request.handle_break(call).await;
                continue 'operation;
            }
            input.write(call, request, byte[0]).await;
        }
        return Err(ServiceError::UnterminatedString);
    }
}

/// AH=0Ah edits a line; buffer capacity includes the final carriage return.
#[interrupt(context(call), args(buffer = ptr(ds, dx)), returns {
    Ok(()) => {},
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) async fn read_buffered(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    buffer: FarPointer,
) -> Result<(), DosError> {
    loop {
        let input = Input::start(request).await;
        let capacity = call.memory.read_u8(buffer.address()).unwrap();
        if capacity == 0 {
            return Ok(());
        }
        let ControlFlow::Continue(bytes) = input.read_line(call, request, capacity).await? else {
            continue;
        };
        // DOS's capacity includes CR, but the returned count excludes it.
        call.memory
            .write_wrapping(buffer.wrapping_add(1), &[(bytes.len() - 1) as u8])
            .unwrap();
        call.memory
            .write_wrapping(buffer.wrapping_add(2), &bytes)
            .unwrap();
        return Ok(());
    }
}

/// AH=0Bh reports stdin availability without consuming ordinary input.
#[interrupt(context(call), returns { status => { al = status } })]
pub(super) async fn input_status(call: &mut CpuContext<'_>, request: &mut Request<'_>) -> u8 {
    loop {
        let input = Input::new(request.services);
        if input.break_pending().await {
            request.handle_break(call).await;
            continue;
        }
        return if input.status(call, request).await {
            0xff
        } else {
            0
        };
    }
}

/// AH=0Ch selects an input service. Input::start flushes before each attempt,
/// so a returning break handler repeats the flush as well as the selected read.
#[interrupt(context(call), args(function = al))]
pub(super) async fn flush_input(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    function: u8,
) {
    interrupt_dispatch!(
        call,
        match function {
            0x01 => read_character_echo(request),
            0x06 => direct_io(request),
            0x07 => read_character(request),
            0x08 => read_character_check_break(request),
            0x0a => read_buffered(request),
            _ => flush_only(request),
        }
    );
}

#[interrupt(returns { () => { al = 0u8 } })]
async fn flush_only(request: &Request<'_>) {
    Input::start(request).await;
}
