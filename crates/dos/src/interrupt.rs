//! Register DOS interrupts and bind each invocation to its active process.

use crate::{error::DosError, process::ProcessStatus, services, state::DosState};
use relocade_machine::{
    real_mode_interrupt::{self, interrupt_handler, Interrupts},
    CpuContext, FarPointer,
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
const INT23_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x120,
};
const INT1B_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x130,
};

pub(crate) fn register_interrupts(interrupts: &mut Interrupts<'_>, session: Rc<DosState>) {
    let console = session.clone();
    interrupts.register_interrupt(0x1b, INT1B_ENTRY, async move |_| {
        console.control_break();
    });
    interrupts.register_interrupt(
        0x23,
        INT23_ENTRY,
        for_process(session.clone(), services::int20),
    );
    interrupts.register_interrupt(
        0x20,
        INT20_ENTRY,
        for_process(session.clone(), services::int20),
    );
    interrupts.register_interrupt(
        0x21,
        INT21_ENTRY,
        for_process(session, interrupt_handler!(services::int21)),
    );
}

/// Resolves and retains the active process for each interrupt invocation.
/// Pending device requests admit IRQs. Ready services finish in the same poll;
/// normal completion restores live IF, while abandonment preserves guest state.
fn for_process<F>(session: Rc<DosState>, handler: F) -> impl AsyncFn(&mut CpuContext<'_>)
where
    F: AsyncFn(&mut CpuContext<'_>, &services::Services<'_>) + 'static,
{
    async move |context| {
        let process = session.process().expect("DOS interrupt requires a process");
        assert_eq!(
            process.status(),
            ProcessStatus::Active,
            "DOS interrupt requires an active process"
        );
        let services = services::Services {
            dos: &session,
            process: &process,
        };
        real_mode_interrupt::with_interrupts_enabled(context.cpu, handler(context, &services)).await
    }
}

/// Shared DOS error ABI; successful services declare their own outputs and flags.
#[real_mode_interrupt::interrupt(returns {
    error @ DosError::InsufficientMemory { available } => { ax = error.code(), bx = available, cf = true },
    error => { ax = error.code(), cf = true },
})]
pub(crate) fn return_error(error: DosError) -> DosError {
    error
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
