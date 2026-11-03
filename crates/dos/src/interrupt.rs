//! Register DOS interrupts and bind each invocation to its active process.

use crate::{
    error::DosError,
    process::{Process, ProcessStatus},
    services,
    state::DosState,
    ServiceError,
};
use relocade_machine::{
    real_mode_interrupt::{self, Interrupts},
    CpuContext, CpuStateAccess, FarPointer, MemoryView,
};
use std::rc::Rc;

const INT20_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x100,
};
const INT21_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x110,
};

pub(crate) fn register_interrupts(interrupts: &mut Interrupts<'_>, session: Rc<DosState>) {
    interrupts.register_interrupt(
        0x20,
        INT20_ENTRY,
        for_process(session.clone(), services::int20),
    );
    interrupts.register_interrupt(0x21, INT21_ENTRY, for_process(session, services::int21));
}

/// One CPU invocation bound to its process and shared DOS resources.
/// Resource borrows last for each operation, not the whole invocation.
pub(crate) struct DosContext<'call> {
    pub(crate) cpu: &'call CpuStateAccess,
    pub(crate) memory: MemoryView<'call>,
    pub(crate) dos: &'call DosState,
    pub(crate) process: &'call Process,
}

impl<'call> DosContext<'call> {
    pub(crate) fn stdout(&self) -> crate::console::CharacterOutput<'call> {
        crate::console::CharacterOutput::handle(
            self.dos.stdio(),
            self.process.files().writable_handle(1),
        )
    }

    /// Records process termination and returns to the embedding after this host poll.
    pub(crate) fn exit(&mut self, status: Result<u8, ServiceError>) {
        self.process
            .terminate(status, &mut self.memory, &mut self.dos.filesystem());
        self.dos.request_exit();
    }
}

/// Resolves and retains the active process for each interrupt invocation.
/// Pending device requests admit IRQs. Ready services finish in the same poll;
/// normal completion restores live IF, while abandonment preserves guest state.
fn for_process<F>(session: Rc<DosState>, handler: F) -> impl AsyncFn(&mut CpuContext<'_>)
where
    F: AsyncFn(&mut DosContext<'_>) + 'static,
{
    async move |context| {
        let process = session.process().expect("DOS interrupt requires a process");
        assert_eq!(
            process.status(),
            ProcessStatus::Active,
            "DOS interrupt requires an active process"
        );
        let mut call = DosContext {
            cpu: context.cpu,
            memory: session.memory().view(),
            dos: &session,
            process: &process,
        };
        real_mode_interrupt::with_interrupts_enabled(context.cpu, handler(&mut call)).await
    }
}

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(call: &mut DosContext<'_>, result: Result<(), DosError>) {
    if let Err(error) = result {
        call.cpu.set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.cpu.set_bx(available);
        }
    }
    real_mode_interrupt::set_carry(call.cpu, &mut call.memory, result.is_err());
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
