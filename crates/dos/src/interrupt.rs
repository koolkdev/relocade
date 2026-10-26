//! Bind DOS state to CPU hooks and complete real-mode interrupt calls.

use crate::{error::DosError, state::DosState};
use relocade_machine::{real_mode_interrupt, CpuContext, Hook, HookResult};
use std::{cell::RefCell, future::Future, rc::Rc};

/// One CPU execution context and DOS state borrowed for a service invocation.
pub(crate) struct DosContext<'call, 'cpu> {
    pub(crate) execution: &'call mut CpuContext<'cpu>,
    pub(crate) dos: &'call mut DosState,
    session: &'call Rc<RefCell<DosState>>,
}

impl DosContext<'_, '_> {
    /// Waits without borrowing DOS state, then binds it to the continuation.
    /// A continuation may defer again while keeping the current interrupt frame.
    pub(crate) fn defer<T: 'static>(
        &mut self,
        input: impl Future<Output = T> + 'static,
        resume: impl FnOnce(&mut DosContext<'_, '_>, T) -> HookResult + 'static,
    ) -> HookResult {
        let session = Rc::clone(self.session);
        self.execution.defer(async move |execution| {
            let value = input.await;
            invoke(execution, &session, |call| resume(call, value))
        })
    }
}

/// Captures the session once, borrowing its state only while a service runs.
pub(crate) fn hook(
    address: u32,
    session: Rc<RefCell<DosState>>,
    handler: fn(&mut DosContext<'_, '_>) -> HookResult,
) -> Hook {
    Hook::new(address, move |execution| {
        invoke(execution, &session, handler)
    })
}

fn invoke(
    execution: &mut CpuContext<'_>,
    session: &Rc<RefCell<DosState>>,
    handler: impl FnOnce(&mut DosContext<'_, '_>) -> HookResult,
) -> HookResult {
    let mut state = session.borrow_mut();
    handler(&mut DosContext {
        execution,
        dos: &mut state,
        session,
    })
}

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(
    call: &mut DosContext<'_, '_>,
    result: Result<(), DosError>,
) -> HookResult {
    if let Err(error) = result {
        call.execution.cpu.set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.execution.cpu.set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(call.execution, result.is_err());
    real_mode_interrupt::return_from_interrupt(call.execution)
}
