//! Helpers for hooks entered through a 16-bit real-mode interrupt frame.
//!
//! Place [`HOOK_STUB`] in guest memory and register it with [`hook`].
//! A returning handler automatically continues at the guest
//! IRET, which restores IP, CS and FLAGS. Handlers can update saved flags with
//! [`set_carry`] or [`set_zero`] before returning.

use super::Hook;
use crate::{CpuStateAccess, MemoryView};
use relocade_x86::Segment;
use std::future::Future;

/// Entry NOP, completion IRET, then resume NOP in an existing service stub.
/// Only entry and resume are intercepted; completion executes the guest IRET.
pub const HOOK_STUB: [u8; 3] = [0x90, 0xcf, 0x90];

/// Registers the service's positions in [`HOOK_STUB`]. Normal handler completion
/// executes IRET; a [`super::StopHandle`] request returns from execution instead.
pub fn hook<F>(address: u32, handler: F) -> Hook
where
    F: AsyncFn() + 'static,
{
    Hook::new(address, handler).with_continuations(
        address.checked_add(2).expect("stub address"),
        address.checked_add(1).expect("stub address"),
    )
}

/// Enables live IF for an ordinary composed operation, restoring the previous
/// IF only after normal completion. Wrap the whole operation, including any
/// selection or cancellation of its inner futures. Dropping this wrapper leaves
/// live CPU state untouched; abandonment must not restore an old call's IF into
/// guest ISR execution or a nested invocation. No backend borrow is retained.
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
