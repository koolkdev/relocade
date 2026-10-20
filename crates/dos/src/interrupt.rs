use crate::{error::DosError, DosState};
use relocade_runtime::{real_mode_interrupt, HostCall};
use relocade_x86::ExecutionExit;
use std::ops::ControlFlow;

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(
    call: &mut HostCall<'_, DosState>,
    result: Result<(), DosError>,
) -> ControlFlow<ExecutionExit> {
    if let Err(error) = result {
        call.cpu().set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.cpu().set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(call, result.is_err());
    real_mode_interrupt::return_from_interrupt(call)
}
