use super::return_to_host;
use crate::{interrupt::return_result, DosState, ServiceError};
use relocade_runtime::{
    real_mode_interrupt::{return_from_interrupt, set_zero},
    HostCall,
};
use relocade_x86::{ExecutionExit, Segment};
use std::ops::ControlFlow;

mod line;
use line::LineInput;

/// AH=06h: DL=FFh polls stdin without echo; other values write a byte to stdout.
pub(super) fn direct_io(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let byte = call.cpu().dl();
    if byte == 0xff {
        let result = call.state_mut().files.read_character(0);
        let character = match result {
            Ok(character) => character,
            Err(error) => return return_result(call, Err(error)),
        };
        call.cpu().set_al(character.unwrap_or(0));
        set_zero(call, character.is_none());
    } else {
        // Like AH=09h, character output has no write-error return.
        let _ = call.state_mut().files.write(1, &[byte]);
        call.cpu().set_al(byte);
    }
    return_from_interrupt(call)
}

/// AH=07h: read stdin without echo or Ctrl-C processing, waiting if necessary.
pub(super) fn read_character(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let result = call.state_mut().files.read_character(0);
    match result {
        Ok(Some(byte)) => return_character(call, byte),
        Ok(None) => match call.state().files.wait_for_character(0) {
            Ok(input) => call.defer(async move |call| {
                let byte = input.await;
                return_character(call, byte)
            }),
            Err(error) => return_result(call, Err(error)),
        },
        Err(error) => return_result(call, Err(error)),
    }
}

fn return_character(call: &mut HostCall<'_, DosState>, byte: u8) -> ControlFlow<ExecutionExit> {
    call.cpu().set_al(byte);
    return_from_interrupt(call)
}

/// AH=0Ah: edit a line into DS:DX's buffer, whose capacity includes the final CR.
pub(super) fn read_buffered(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let base = call.cpu().segment(Segment::Ds).base;
    let offset = call.cpu().dx();
    let mut capacity = [0];
    call.memory()
        .read(base + u32::from(offset), &mut capacity)
        .expect("real-mode line buffer");
    if capacity[0] == 0 {
        return return_from_interrupt(call);
    }
    let mut line = LineInput::new(capacity[0]);
    match line.read_available(&mut call.state_mut().files) {
        Ok(true) => return_line(call, base, offset, &line.bytes),
        Ok(false) => call.defer(async move |call| {
            match line.read_remaining(&mut call.state_mut().files).await {
                Ok(()) => return_line(call, base, offset, &line.bytes),
                Err(error) => return_result(call, Err(error)),
            }
        }),
        Err(error) => return_result(call, Err(error)),
    }
}

fn return_line(
    call: &mut HostCall<'_, DosState>,
    base: u32,
    offset: u16,
    bytes: &[u8],
) -> ControlFlow<ExecutionExit> {
    let count = (bytes.len() - 1) as u8; // DOS's count excludes the carriage return.
    for (index, byte) in std::iter::once(&count).chain(bytes).enumerate() {
        call.memory()
            .write(
                base + u32::from(offset.wrapping_add(1 + index as u16)),
                &[*byte],
            )
            .expect("real-mode line buffer");
    }
    return_from_interrupt(call)
}

/// AH=09h: output the '$'-terminated string at DS:DX and return AL='$'.
pub(super) fn write_string(call: &mut HostCall<'_, DosState>) -> ControlFlow<ExecutionExit> {
    let base = call.cpu().segment(Segment::Ds).base;
    let start = call.cpu().dx();
    for index in 0..=u16::MAX {
        let mut byte = [0];
        call.memory()
            .read(base + u32::from(start.wrapping_add(index)), &mut byte)
            .expect("real-mode byte address");
        if byte[0] == b'$' {
            call.cpu().set_al(b'$');
            return return_from_interrupt(call);
        }
        // AH=09h writes through stdout, without returning a write-error status.
        let _ = call.state_mut().files.write(1, &byte);
    }
    return_to_host(call, Err(ServiceError::UnterminatedString))
}
