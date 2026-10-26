//! DOS version and interrupt-vector services used during program startup.

use crate::DosServices;
use relocade_machine::{real_mode_interrupt::return_from_interrupt, CpuContext, HookResult};
use relocade_x86::{Segment, SegmentRegister};

/// AH=30h: report DOS 5.00, with no serial number or special version flags.
pub(super) fn version(call: &mut CpuContext<'_, DosServices>) -> HookResult {
    let cpu = &mut call.cpu;
    let version_flags = cpu.al() == 1;
    cpu.set_ax(5); // AL is the major version, AH the minor version.
    cpu.set_bx(if version_flags { 0 } else { 0xff00 }); // FFh is the Microsoft OEM ID.
    cpu.set_cx(0);
    return_from_interrupt(call)
}

/// AH=25h: set AL's interrupt vector to DS:DX.
pub(super) fn set_vector(call: &mut CpuContext<'_, DosServices>) -> HookResult {
    let cpu = &call.cpu;
    let address = u32::from(cpu.al()) * 4;
    let [offset_lo, offset_hi] = cpu.dx().to_le_bytes();
    let [segment_lo, segment_hi] = cpu.segment(Segment::Ds).selector.to_le_bytes();
    call.memory
        .write(address, &[offset_lo, offset_hi, segment_lo, segment_hi])
        .expect("interrupt vector table");
    return_from_interrupt(call)
}

/// AH=35h: return AL's current interrupt vector in ES:BX.
pub(super) fn get_vector(call: &mut CpuContext<'_, DosServices>) -> HookResult {
    let address = u32::from(call.cpu.al()) * 4;
    let mut vector = [0; 4];
    call.memory
        .read(address, &mut vector)
        .expect("interrupt vector table");
    let offset = u16::from_le_bytes([vector[0], vector[1]]);
    let segment = u16::from_le_bytes([vector[2], vector[3]]);
    let cpu = &mut call.cpu;
    cpu.set_bx(offset);
    cpu.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, segment),
    );
    return_from_interrupt(call)
}
