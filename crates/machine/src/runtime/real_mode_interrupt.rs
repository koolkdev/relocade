//! Helpers for hooks entered through a 16-bit real-mode interrupt frame.
//!
//! Place [`HOOK_STUB`] in guest memory and register its first byte with [`hook`].
//! When the async handler returns, the adapter advances to the guest IRET,
//! which restores IP, CS and FLAGS. Handlers can update saved flags with
//! [`set_carry`] or [`set_zero`] before returning.

use super::Hook;
use crate::{CpuStateAccess, MemoryView};
use relocade_x86::Segment;

/// NOP reserves the hook entry; IRET performs the interrupt return in the guest.
pub const HOOK_STUB: [u8; 2] = [0x90, 0xcf];

/// Adapts an ordinary async handler to the real-mode interrupt return convention.
/// The handler sees the hook's entry CS:IP until it returns. It must leave that
/// position and the interrupt frame intact, apart from intended frame edits.
/// Completion advances IP once; the guest executes IRET on the next dispatch.
/// Pending handlers retain their future, and dropping one does not advance IP.
pub fn hook<F>(address: u32, cpu: CpuStateAccess, handler: F) -> Hook
where
    F: AsyncFn() + 'static,
{
    Hook::new(address, async move || {
        handler().await;
        let iret_ip = (cpu.eip() as u16).wrapping_add(1);
        cpu.set_eip(u32::from(iret_ip));
    })
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
