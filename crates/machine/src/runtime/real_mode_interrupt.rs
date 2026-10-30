//! Helpers for hooks entered through a 16-bit real-mode interrupt frame.
//!
//! Place [`HOOK_STUB`] in guest memory and register its pair with [`hook`].
//! A returning handler uses [`return_from_interrupt`] to resume at the guest
//! IRET, which restores IP, CS and FLAGS. Handlers can update saved flags with
//! [`set_carry`] or [`set_zero`] before returning.

use super::{CallContext, Hook, HookResult};
use crate::{CpuAccess, CpuView, MemoryView};
use relocade_x86::Segment;

/// Entry NOP, resume NOP, then the guest IRET used on completion.
/// The first two positions are distinct registered dispatch entries.
pub const HOOK_STUB: [u8; 3] = [0x90, 0x90, 0xcf];

/// Registers the entry/resume labels in a real-mode service's [`HOOK_STUB`].
pub fn hook<T: 'static, F>(address: u32, handler: F) -> Hook<T>
where
    F: AsyncFn(CallContext<T>) -> HookResult + 'static,
{
    Hook::new(
        address,
        address.checked_add(1).expect("stub address"),
        handler,
    )
}

/// Changes carry in the saved FLAGS, preserving every other flag.
/// The current SS:SP must point to the IP pushed by a 16-bit real-mode interrupt.
pub fn set_carry(cpu: &CpuView<impl CpuAccess>, memory: &mut MemoryView<'_>, carry: bool) {
    set_flag(cpu, memory, 1, carry);
}

/// Changes zero in the saved FLAGS, preserving every other flag.
/// The current SS:SP must point to the IP pushed by a 16-bit real-mode interrupt.
pub fn set_zero(cpu: &CpuView<impl CpuAccess>, memory: &mut MemoryView<'_>, zero: bool) {
    set_flag(cpu, memory, 1 << 6, zero);
}

fn set_flag(cpu: &CpuView<impl CpuAccess>, memory: &mut MemoryView<'_>, mask: u8, value: bool) {
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

/// Resumes at the IRET in [`HOOK_STUB`], preserving the interrupt frame.
/// The current CS:IP must point to the stub's resume label.
pub fn return_from_interrupt(cpu: &mut CpuView<impl CpuAccess>) -> HookResult {
    // The finish label is contiguous with resume. Preserve an out-of-limit EIP
    // so ordinary guest fetch faults instead of wrapping into unrelated code.
    cpu.set_eip(cpu.eip().wrapping_add(1));
    HookResult::Continue
}
