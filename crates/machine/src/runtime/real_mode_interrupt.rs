//! Helpers for hooks entered through a 16-bit real-mode interrupt frame.
//!
//! Place [`HOOK_STUB`] in guest memory and install a hook at its first byte.
//! A returning handler uses [`return_from_interrupt`] to resume at the guest
//! IRET, which restores IP, CS and FLAGS. Handlers can update saved flags with
//! [`set_carry`] or [`set_zero`] before returning.

use super::HostCall;
use relocade_x86::{ExecutionExit, Segment};
use std::ops::ControlFlow;

/// NOP reserves the hook entry; IRET performs the interrupt return in the guest.
pub const HOOK_STUB: [u8; 2] = [0x90, 0xcf];

/// Changes carry in the saved FLAGS, preserving every other flag.
/// The current SS:SP must point to the IP pushed by a 16-bit real-mode interrupt.
pub fn set_carry<T>(call: &mut HostCall<'_, T>, carry: bool) {
    set_flag(call, 1, carry);
}

/// Changes zero in the saved FLAGS, preserving every other flag.
/// The current SS:SP must point to the IP pushed by a 16-bit real-mode interrupt.
pub fn set_zero<T>(call: &mut HostCall<'_, T>, zero: bool) {
    set_flag(call, 1 << 6, zero);
}

fn set_flag<T>(call: &mut HostCall<'_, T>, mask: u8, value: bool) {
    // INT pushed FLAGS, CS, IP. Updating live flags would be undone by IRET.
    let cpu = call.cpu();
    let address = cpu.segment(Segment::Ss).base + u32::from(cpu.sp().wrapping_add(4));
    let mut flags = [0];
    call.memory()
        .read(address, &mut flags)
        .expect("real-mode interrupt frame");
    flags[0] = (flags[0] & !mask) | if value { mask } else { 0 };
    call.memory()
        .write(address, &flags)
        .expect("real-mode interrupt frame");
}

/// Resumes at the IRET in [`HOOK_STUB`], preserving the interrupt frame.
/// The current CS:IP must point to the stub's hook entry.
pub fn return_from_interrupt<T>(call: &mut HostCall<'_, T>) -> ControlFlow<ExecutionExit> {
    let mut cpu = call.cpu();
    let iret_ip = (cpu.eip() as u16).wrapping_add(1);
    cpu.set_eip(u32::from(iret_ip));
    ControlFlow::Continue(())
}
