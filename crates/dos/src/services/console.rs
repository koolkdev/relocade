use super::return_to_host;
use crate::{DosState, ServiceError};
use std::ops::ControlFlow;
use wasm86_runtime::{real_mode_interrupt::return_from_interrupt, HostCall};
use wasm86_x86::Segment;

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
