//! Bind DOS state to CPU hooks and complete real-mode interrupt calls.

use crate::{
    error::DosError,
    process::{Process, ProcessStatus},
    state::DosState,
    ServiceError,
};
use relocade_machine::{real_mode_interrupt, CallContext, Hook, HookResult};
use relocade_x86::{ExecutionExit, HostExit};
use std::rc::Rc;

/// A DOS invocation owns CPU access and retains its originally selected process.
pub(crate) struct DosContext {
    pub(crate) execution: CallContext,
    pub(crate) dos: Rc<DosState>,
    pub(crate) process: Rc<Process>,
}

impl DosContext {
    pub(crate) fn console(&self) -> crate::services::console::Console<'_> {
        crate::services::console::Console {
            stdio: self.dos.stdio(),
            memory: &self.execution.memory,
            io: &self.execution.io,
        }
    }

    pub(crate) fn stdout(&self) -> crate::services::console::CharacterOutput<'_> {
        self.console()
            .output(self.process.files().writable_handle(1))
    }

    /// Terminates the process and returns from DOS execution.
    pub(crate) fn exit(&mut self, status: Result<u8, ServiceError>) -> HookResult {
        self.process.terminate(
            status,
            &mut self.execution.memory(),
            &mut self.dos.filesystem(),
        );
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    }
}

/// Captures the session and resolves its active process at each hook entry.
pub(crate) fn hook<F>(address: u32, session: Rc<DosState>, handler: F) -> Hook
where
    F: AsyncFn(&mut DosContext) -> HookResult + 'static,
{
    real_mode_interrupt::hook(address, async move |execution| {
        let process = session.process().expect("DOS hook requires a process");
        assert_eq!(
            process.status(),
            ProcessStatus::Active,
            "DOS hook requires an active process"
        );
        let mut call = DosContext {
            execution,
            dos: session.clone(),
            process,
        };
        handler(&mut call).await
    })
}

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(call: &mut DosContext, result: Result<(), DosError>) -> HookResult {
    if let Err(error) = result {
        call.execution.cpu.set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.execution.cpu.set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(
        &call.execution.cpu,
        &mut call.execution.memory(),
        result.is_err(),
    );
    real_mode_interrupt::return_from_interrupt(&mut call.execution.cpu)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
