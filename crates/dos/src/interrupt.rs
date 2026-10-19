use crate::{error::DosError, DosState};
use std::ops::ControlFlow;
use wasm86_runtime::{real_mode_interrupt, HostCall};

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(
    call: &mut HostCall<'_, DosState>,
    result: Result<(), DosError>,
) -> ControlFlow<()> {
    if let Err(error) = result {
        call.cpu().set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.cpu().set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(call, result.is_err());
    real_mode_interrupt::return_from_interrupt(call)
}
