//! Register DOS interrupts and bind each invocation to its active process.

use crate::{error::DosError, process::ProcessStatus, services, state::DosState};
use relocade_machine::{
    real_mode_interrupt::{self, interrupt_handler, Interrupts},
    CpuContext, FarPointer,
};
use std::rc::Rc;

/// Program termination (INT 20h).
pub const TERMINATE: u8 = 0x20;
/// DOS service dispatcher (INT 21h).
pub const SERVICES: u8 = 0x21;
/// Parent continuation saved in the PSP (INT 22h).
pub const TERMINATION_ADDRESS: u8 = 0x22;
/// Ctrl-C/Ctrl-Break callback (INT 23h).
pub const CONTROL_BREAK: u8 = 0x23;
/// Critical-error callback (INT 24h).
pub const CRITICAL_ERROR: u8 = 0x24;
/// Idle callback during DOS character I/O (INT 28h).
pub const IDLE: u8 = 0x28;

const TERMINATE_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x100,
};
const SERVICES_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x110,
};
const CONTROL_BREAK_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x120,
};
const KEYBOARD_BREAK_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x130,
};
pub(crate) const IDLE_ENTRY: FarPointer = FarPointer {
    segment: 0xf000,
    offset: 0x140,
};

pub(crate) fn register_interrupts(interrupts: &mut Interrupts<'_>, session: Rc<DosState>) {
    interrupts.register_interrupt(IDLE, IDLE_ENTRY, async |_| {});
    let console = session.clone();
    interrupts.register_interrupt(
        relocade_bios::interrupts::CONTROL_BREAK,
        KEYBOARD_BREAK_ENTRY,
        async move |_| {
            console.control_break();
        },
    );
    interrupts.register_interrupt(
        CONTROL_BREAK,
        CONTROL_BREAK_ENTRY,
        for_process(session.clone(), services::int20),
    );
    interrupts.register_interrupt(
        TERMINATE,
        TERMINATE_ENTRY,
        for_process(session.clone(), services::int20),
    );
    interrupts.register_interrupt(
        SERVICES,
        SERVICES_ENTRY,
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
