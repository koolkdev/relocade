use crate::{error::DosError, state::DosState};
use relocade_machine::{real_mode_interrupt, CpuContext, HostAction};
use std::{future::Future, rc::Rc};

/// Waits without borrowing DOS files, then resumes the same interrupt frame.
/// The owned continuation keeps its environment alive and may wait again.
pub(crate) fn defer<T: 'static>(
    state: &Rc<DosState>,
    input: impl Future<Output = T> + 'static,
    resume: impl FnOnce(&mut CpuContext<'_>, &Rc<DosState>, T) -> HostAction + 'static,
) -> HostAction {
    let state = Rc::clone(state);
    HostAction::wait(async move |context| {
        let value = input.await;
        resume(context, &state, value)
    })
}

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(call: &mut CpuContext<'_>, result: Result<(), DosError>) -> HostAction {
    if let Err(error) = result {
        call.cpu().set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.cpu().set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(call, result.is_err());
    real_mode_interrupt::return_from_interrupt(call)
}
