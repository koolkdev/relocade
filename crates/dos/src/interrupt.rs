//! Bind DOS state to CPU hooks and complete real-mode interrupt calls.

use crate::{
    error::DosError,
    process::{Process, ProcessStatus},
    state::DosState,
    ServiceError,
};
use relocade_machine::{real_mode_interrupt, CpuStateAccess, Hook, HookResult, MemoryView};
use relocade_x86::{ExecutionExit, HostExit};
use std::rc::Rc;

/// One CPU invocation bound to its process and shared DOS resources.
/// Resource borrows last for each operation, not the whole invocation.
pub(crate) struct DosContext<'call> {
    pub(crate) cpu: &'call CpuStateAccess,
    pub(crate) memory: MemoryView<'call>,
    pub(crate) dos: &'call DosState,
    pub(crate) process: &'call Process,
}

impl<'call> DosContext<'call> {
    pub(crate) fn console(&self) -> crate::Console<'call> {
        crate::Console {
            stdio: self.dos.stdio(),
            memory: self.dos.memory(),
        }
    }

    pub(crate) fn stdout(&self) -> crate::console::CharacterOutput<'call> {
        self.console()
            .output(self.process.files().writable_handle(1))
    }

    /// Terminates the process and returns from DOS execution.
    pub(crate) fn exit(&mut self, status: Result<u8, ServiceError>) -> HookResult {
        self.process
            .terminate(status, &mut self.memory, &mut self.dos.filesystem());
        HookResult::Exit(ExecutionExit::Host(HostExit::Return))
    }
}

/// Captures the session and resolves its active process at each hook entry.
pub(crate) fn hook<F>(address: u32, cpu: CpuStateAccess, session: Rc<DosState>, handler: F) -> Hook
where
    F: AsyncFn(&mut DosContext<'_>) -> HookResult + 'static,
{
    real_mode_interrupt::hook(address, async move || {
        let process = session.process().expect("DOS hook requires a process");
        assert_eq!(
            process.status(),
            ProcessStatus::Active,
            "DOS hook requires an active process"
        );
        let mut call = DosContext {
            cpu: &cpu,
            memory: session.memory().view(),
            dos: &session,
            process: &process,
        };
        handler(&mut call).await
    })
}

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(call: &mut DosContext<'_>, result: Result<(), DosError>) -> HookResult {
    if let Err(error) = result {
        call.cpu.set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.cpu.set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(call.cpu, &mut call.memory, result.is_err());
    HookResult::Complete
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
