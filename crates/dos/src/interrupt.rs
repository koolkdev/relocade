use crate::{error::DosError, state::DosState, DosServices};
use relocade_machine::{real_mode_interrupt, CpuContext, Hook, HookResult};
use std::future::Future;

/// Borrows the DOS environment for an interrupt and its nested services.
pub(crate) fn hook(
    address: u32,
    handler: fn(&mut CpuContext<'_, DosServices>, &mut DosState) -> HookResult,
) -> Hook<DosServices> {
    Hook::new(address, move |call| invoke(call, handler))
}

fn invoke(
    call: &mut CpuContext<'_, DosServices>,
    handler: impl FnOnce(&mut CpuContext<'_, DosServices>, &mut DosState) -> HookResult,
) -> HookResult {
    let services = call.state.clone();
    let mut state = services.borrow_mut();
    handler(call, &mut state)
}

/// Waits without borrowing DOS state, then lends it to the continuation.
/// A continuation may defer again while keeping the current interrupt frame.
pub(crate) fn defer<T: 'static>(
    call: &mut CpuContext<'_, DosServices>,
    input: impl Future<Output = T> + 'static,
    resume: impl FnOnce(&mut CpuContext<'_, DosServices>, &mut DosState, T) -> HookResult + 'static,
) -> HookResult {
    call.defer(async move |call| {
        let value = input.await;
        invoke(call, |call, state| resume(call, state, value))
    })
}

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(
    call: &mut CpuContext<'_, DosServices>,
    result: Result<(), DosError>,
) -> HookResult {
    if let Err(error) = result {
        call.cpu.set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.cpu.set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(call, result.is_err());
    real_mode_interrupt::return_from_interrupt(call)
}
