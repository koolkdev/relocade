//! Bind DOS state to CPU hooks and complete real-mode interrupt calls.

use crate::{
    error::DosError,
    process::{Process, ProcessStatus},
    state::DosState,
    ServiceError,
};
use relocade_machine::{real_mode_interrupt, CpuContext, Hook, HookResult};
use relocade_x86::{ExecutionExit, HostExit};
use std::rc::Rc;

/// One CPU invocation bound to its process and shared DOS resources.
/// Resource borrows last for each operation, not the whole invocation.
pub(crate) struct DosContext<'call, 'cpu> {
    pub(crate) execution: &'call mut CpuContext<'cpu>,
    pub(crate) dos: &'call DosState,
    pub(crate) process: &'call Process,
}

impl DosContext<'_, '_> {
    pub(crate) fn console(&self) -> crate::Console<'_> {
        crate::Console {
            stdio: self.dos.stdio(),
            memory: self.dos.memory(),
        }
    }

    pub(crate) fn stdout(&self) -> crate::console::CharacterOutput<'_> {
        self.console()
            .output(self.process.files().writable_handle(1))
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
pub(crate) fn hook<F>(address: u32, session: Rc<DosState>, handler: F) -> Hook
where
    F: AsyncFn(&mut DosContext<'_, '_>) -> HookResult + 'static,
{
    Hook::new(address, async move |execution| {
        let process = session.process().expect("DOS hook requires a process");
        assert_eq!(
            process.status(),
            ProcessStatus::Active,
            "DOS hook requires an active process"
        );
        let mut call = DosContext {
            execution,
            dos: &session,
            process: &process,
        };
        handler(&mut call).await
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
