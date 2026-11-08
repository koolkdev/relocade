//! Real-mode service registration and interrupt-frame helpers.
//!
//! [`Interrupts`] registers vectors, guest stubs and their handlers together,
//! including far procedures that return with RETF.
//! Low-level adapters can place [`HOOK_STUB`] and install entries from [`hook`].
//! When an interrupt handler returns, the adapter advances to the guest IRET,
//! which restores IP, CS and FLAGS. Far procedures instead complete through RETF.
//! Interrupt handlers can update saved flags with [`set_carry`] or [`set_zero`].
//!
//! The second byte resumes the top suspended host call after a device IRQ or an
//! awaited [`crate::CpuContext::call_interrupt`] handler returns.

mod bindings;
mod registration;
#[doc(hidden)]
pub use crate::cpu::registers;
pub use crate::FarPointer;
pub use bindings::{interrupt, interrupt_dispatch, interrupt_handler};
pub use registration::Interrupts;
pub use relocade_x86::real_mode::{vector_address, InterruptFrame};
#[doc(hidden)]
pub use relocade_x86::{Segment, SegmentRegister};

use super::Hook;
use crate::{CpuStateAccess, MemoryView};
use relocade_x86::{abi, ExecutionExitValues, HostExit};
use std::future::Future;

/// Entry NOP, resume NOP, and completion IRET inside the environment's service code.
/// The adapter supplies all three positions in a batch. The return
/// entry uses the existing interpreter to execute the live guest IRET byte.
pub const HOOK_STUB: [u8; 3] = [0x90, 0x90, 0xcf];

/// Adapts an ordinary async handler to a real-mode service entry.
/// Entry advances IP to the resume position before polling the handler. The
/// handler must preserve that continuation and the return frame, apart from
/// intended frame edits. Completion advances the live IP to the return instruction;
/// dropping a pending handler leaves the CPU untouched. The stub must fit in the
/// entering code segment. Redirected resumes must have the matching return immediately after
/// the live resume position; completion follows that position through the live CS.
/// IRQ entry saves the live resume position; the driver resumes the retained
/// future without restoring any CPU state.
pub fn hook<F>(address: u32, cpu: CpuStateAccess, handler: F) -> [Hook; 3]
where
    F: AsyncFn() + 'static,
{
    let resume = address.checked_add(1).expect("service stub address");
    let return_entry = resume.checked_add(1).expect("service stub address");
    [
        Hook::host(address, async move || {
            let resume_ip = (cpu.eip() as u16).wrapping_add(1);
            cpu.set_eip(u32::from(resume_ip));
            handler().await;
            let return_ip = (cpu.eip() as u16).wrapping_add(1);
            cpu.set_eip(u32::from(return_ip));
        }),
        Hook::block(resume, |program| {
            program
                .function::<abi::ExecutionSignature>(|body, ()| {
                    body.return_(ExecutionExitValues::host(HostExit::Resume))
                })
                .into()
        }),
        Hook::block(return_entry, |program| {
            program.import_function(abi::INTERPRET)
        }),
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

/// Reads the live conventional IVT used by BIOS and DOS vector services.
pub fn read_vector(memory: &MemoryView<'_>, vector: u8) -> FarPointer {
    FarPointer::from_packed(
        memory
            .read_u32(vector_address(vector).address())
            .expect("interrupt vector table"),
    )
}

/// Changes a conventional IVT destination without replacing its old handler.
pub fn write_vector(memory: &mut MemoryView<'_>, vector: u8, destination: FarPointer) {
    memory
        .write_u32(vector_address(vector).address(), destination.packed())
        .expect("interrupt vector table");
}

/// Reads a 16-bit interrupt frame at the current SS:SP.
pub fn read_frame(cpu: &CpuStateAccess, memory: &MemoryView<'_>) -> InterruptFrame {
    let mut bytes = [0; InterruptFrame::BYTE_LEN];
    let address = cpu.segment(Segment::Ss).base + u32::from(cpu.sp());
    memory
        .read(address, &mut bytes)
        .expect("real-mode interrupt frame");
    InterruptFrame::from_bytes(bytes)
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
    let address = cpu.segment(Segment::Ss).base
        + u32::from(cpu.sp().wrapping_add(InterruptFrame::FLAGS_OFFSET as u16));
    let mut flags = [0];
    memory
        .read(address, &mut flags)
        .expect("real-mode interrupt frame");
    flags[0] = (flags[0] & !mask) | if value { mask } else { 0 };
    memory
        .write(address, &flags)
        .expect("real-mode interrupt frame");
}
