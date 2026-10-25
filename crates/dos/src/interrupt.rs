use crate::{error::DosError, DosServices};
use relocade_machine::{real_mode_interrupt, CpuContext, HookResult};

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(
    call: &mut CpuContext<'_, DosServices>,
    result: Result<(), DosError>,
) -> HookResult {
    if let Err(error) = result {
        call.cpu().set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.cpu().set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(call, result.is_err());
    real_mode_interrupt::return_from_interrupt(call)
}
