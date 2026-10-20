use super::return_to_host;
use crate::{interrupt::return_result, DosState, ServiceError};
use relocade_runtime::{
    real_mode_interrupt::{return_from_interrupt, set_zero},
    HostCall,
};
use relocade_x86::Segment;
use std::ops::ControlFlow;

/// AH=06h: DL=FFh polls stdin without echo; other values write a byte to stdout.
pub(super) fn direct_io(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
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

/// AH=09h: output the '$'-terminated string at DS:DX and return AL='$'.
pub(super) fn write_string(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
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
