//! Helpers for hooks entered through a 16-bit real-mode interrupt frame.
//!
//! Place [`HOOK_STUB`] in guest memory and register it with [`hook`].
//! A returning handler uses [`HookResult::Complete`] to continue at the guest
//! IRET, which restores IP, CS and FLAGS. Handlers can update saved flags with
//! [`set_carry`] or [`set_zero`] before returning.

use super::{Hook, HookResult};
use crate::{CpuStateAccess, MemoryView};
use relocade_x86::Segment;

/// Entry NOP, completion IRET, then resume NOP in an existing service stub.
/// Only entry and resume are intercepted; completion executes the guest IRET.
pub const HOOK_STUB: [u8; 3] = [0x90, 0xcf, 0x90];

/// Registers the service's positions in [`HOOK_STUB`]. Handlers return
/// [`HookResult::Complete`] for IRET or choose an explicit continuation/exit.
pub fn hook<F>(address: u32, handler: F) -> Hook
where
    F: AsyncFn() -> HookResult + 'static,
{
    Hook::new(address, handler).with_continuations(
        address.checked_add(2).expect("stub address"),
        address.checked_add(1).expect("stub address"),
    )
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
