//! Helpers for hooks entered through a 16-bit real-mode interrupt frame.
//!
//! Place [`HOOK_STUB`] in guest memory and install the entries returned by [`hook`].
//! When the async handler returns, the adapter advances to the guest IRET,
//! which restores IP, CS and FLAGS. Handlers can update saved flags with
//! [`set_carry`] or [`set_zero`] before returning.
//! The second byte resumes the top suspended host call after guest IRQ execution.

use super::{Entry, Hook};
use crate::{CpuStateAccess, MemoryView};
use relocade_x86::{abi, ExecutionExitValues, HostExit, Segment};
use std::future::Future;

/// Entry NOP, resume NOP, and completion IRET inside the environment's service code.
/// The adapter supplies all three positions in a batch. The return
/// entry uses the existing interpreter to execute the live guest IRET byte.
pub const HOOK_STUB: [u8; 3] = [0x90, 0x90, 0xcf];

/// Adapts an ordinary async handler to the real-mode interrupt return convention.
/// Entry advances IP to the resume position before polling the handler. The
/// handler must preserve that continuation and the interrupt frame, apart from
/// intended frame edits. Completion advances the live IP once more to IRET;
/// dropping a pending handler leaves the CPU untouched. The stub must fit in the
/// entering code segment. Redirected resumes must also have IRET immediately after
/// the live resume position; completion follows that position through the live CS.
/// IRQ entry saves the live resume position; the driver resumes the retained
/// future without restoring any CPU state.
pub fn hook<F>(address: u32, cpu: CpuStateAccess, handler: F) -> [Entry; 3]
where
    F: AsyncFn() + 'static,
{
    let iret = address.checked_add(2).expect("interrupt stub address");
    [
        Hook::new(address, async move || {
            let resume_ip = (cpu.eip() as u16).wrapping_add(1);
            cpu.set_eip(u32::from(resume_ip));
            handler().await;
            let iret_ip = (cpu.eip() as u16).wrapping_add(1);
            cpu.set_eip(u32::from(iret_ip));
        })
        .into(),
        Entry::block(address + 1, |program| {
            program
                .function::<abi::ExecutionSignature>(|body, ()| {
                    body.return_(ExecutionExitValues::host(HostExit::Resume))
                })
                .into()
        }),
        Entry::block(iret, |program| program.import_function(abi::INTERPRET)),
    ]
}

/// Enables live IF for an ordinary composed operation, restoring it only after
/// normal completion. Wrap the whole operation, including selection/cancellation
/// of inner futures. Abandoning this wrapper leaves the current CPU untouched;
/// a destructor must not restore an outer call's IF into an ISR or nested call.
pub async fn with_interrupts_enabled<F: Future>(cpu: &CpuStateAccess, future: F) -> F::Output {
    let saved = cpu.interrupts_enabled();
    cpu.set_interrupts_enabled(true);
    let result = future.await;
    cpu.set_interrupts_enabled(saved);
    result
}

/// Changes carry in the saved FLAGS, preserving every other flag.
/// The current SS:SP must point to the IP pushed by a 16-bit real-mode interrupt.
pub fn set_carry(cpu: &CpuStateAccess, memory: &mut MemoryView<'_>, carry: bool) {
    set_flag(cpu, memory, 1, carry);
}

/// Changes zero in the saved FLAGS, preserving every other flag.
/// The current SS:SP must point to the IP pushed by a 16-bit real-mode interrupt.
pub fn set_zero(cpu: &CpuStateAccess, memory: &mut MemoryView<'_>, zero: bool) {
    set_flag(cpu, memory, 1 << 6, zero);
}

fn set_flag(cpu: &CpuStateAccess, memory: &mut MemoryView<'_>, mask: u8, value: bool) {
    // INT pushed FLAGS, CS, IP. Updating live flags would be undone by IRET.
    let address = cpu.segment(Segment::Ss).base + u32::from(cpu.sp().wrapping_add(4));
    let mut flags = [0];
    memory
        .read(address, &mut flags)
        .expect("real-mode interrupt frame");
    flags[0] = (flags[0] & !mask) | if value { mask } else { 0 };
    memory
        .write(address, &flags)
        .expect("real-mode interrupt frame");
}
