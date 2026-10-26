//! Bind DOS state to CPU hooks and complete real-mode interrupt calls.

use crate::{
    error::DosError,
    process::{Process, ProcessStatus},
    state::DosState,
    ServiceError,
};
use relocade_machine::{real_mode_interrupt, CpuContext, Hook, HookResult};
use relocade_x86::{ExecutionExit, HostExit};
use std::{future::Future, rc::Rc};

/// One CPU invocation bound to its process and shared DOS resources.
/// Resource borrows last for each operation, not the whole invocation.
pub(crate) struct DosContext<'call, 'cpu> {
    pub(crate) execution: &'call mut CpuContext<'cpu>,
    pub(crate) dos: &'call Rc<DosState>,
    pub(crate) process: &'call Rc<Process>,
}

impl DosContext<'_, '_> {
    /// Waits without borrowing DOS resources, then binds the same process again.
    /// A continuation may defer again while keeping the current interrupt frame.
    pub(crate) fn defer<T: 'static>(
        &mut self,
        input: impl Future<Output = T> + 'static,
        resume: impl FnOnce(&mut DosContext<'_, '_>, T) -> HookResult + 'static,
    ) -> HookResult {
        let session = Rc::clone(self.dos);
        let process = Rc::clone(self.process);
        self.execution.defer(async move |execution| {
            let value = input.await;
            invoke(execution, &session, &process, |call| resume(call, value))
        })
    }

    /// Terminates the process and returns from DOS execution.
    pub(crate) fn exit(&mut self, status: Result<u8, ServiceError>) -> HookResult {
        self.process.terminate(
            status,
            &mut self.execution.memory,
            &mut self.dos.filesystem(),
        );
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    }
}

/// Captures the session and resolves its active process at each hook entry.
pub(crate) fn hook(
    address: u32,
    session: Rc<DosState>,
    handler: fn(&mut DosContext<'_, '_>) -> HookResult,
) -> Hook {
    Hook::new(address, move |execution| {
        let process = session.process();
        match process {
            Some(process) if process.status() == ProcessStatus::Active => {
                invoke(execution, &session, &process, handler)
            }
            _ => HookResult::Exit(ExecutionExit::Host(HostExit::Return)),
        }
    })
}

fn invoke(
    execution: &mut CpuContext<'_>,
    session: &Rc<DosState>,
    process: &Rc<Process>,
    handler: impl FnOnce(&mut DosContext<'_, '_>) -> HookResult,
) -> HookResult {
    let mut call = DosContext {
        execution,
        dos: session,
        process,
    };
    handler(&mut call)
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
