use crate::{error::DosError, DosState};
use std::ops::ControlFlow;
use wasm86_runtime::HostCall;
use wasm86_x86::Segment;

/// Returns a DOS result through the interrupt frame, preserving other flags.
pub(crate) fn return_result(
    call: &mut HostCall<'_, DosState>,
    result: Result<(), DosError>,
) -> ControlFlow<()> {
    if let Err(error) = result {
        call.cpu().set_ax(error.code());
        if let DosError::InsufficientMemory { available } = error {
            call.cpu().set_bx(available);
        }
    }
    // Real-mode INT pushed FLAGS, CS, IP. Carry must be changed in the saved
    // FLAGS at SS:SP+4; changing live CPU flags would be undone by IRET.
    let cpu = call.cpu();
    let flags_address = cpu.segment(Segment::Ss).base + u32::from(cpu.sp().wrapping_add(4));
    let mut flags = [0];
    call.memory()
        .read(flags_address, &mut flags)
        .expect("interrupt frame");
    flags[0] = (flags[0] & !1) | u8::from(result.is_err());
    call.memory()
        .write(flags_address, &flags)
        .expect("interrupt frame");
    return_from_interrupt(call)
}

pub(crate) fn return_from_interrupt(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    // Advance past the hook's NOP byte. The guest IRET restores IP, CS and FLAGS
    // from the interrupt frame; the host handler does not pop the frame itself.
    let mut cpu = call.cpu();
    let iret_ip = (cpu.eip() as u16).wrapping_add(1);
    cpu.set_eip(u32::from(iret_ip));
    ControlFlow::Continue(())
}
