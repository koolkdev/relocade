use super::files::read_path;
use crate::{
    error::DosError, files::FIND_DATA_BYTES, interrupt::return_result, state::DosState, DosServices,
};
use relocade_machine::{real_mode_interrupt::return_from_interrupt, CpuContext, HookResult};
use relocade_x86::{Segment, SegmentRegister};

/// AH=1Ah: select the disk transfer area at DS:DX, preserving flags.
pub(super) fn set_dta(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let segment = call.cpu.segment(Segment::Ds).selector;
    let offset = call.cpu.dx();
    let process = state.process.active_mut();
    process.dta_segment = segment;
    process.dta_offset = offset;
    return_from_interrupt(call)
}

/// AH=2Fh: return the current DTA in ES:BX, preserving flags.
pub(super) fn get_dta(call: &mut CpuContext<'_, DosServices>, state: &mut DosState) -> HookResult {
    let process = state.process.active();
    call.cpu.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, process.dta_segment),
    );
    call.cpu.set_bx(process.dta_offset);
    return_from_interrupt(call)
}

/// AH=4Eh: find DS:DX's 8.3 pattern with CX's attributes; fill the current DTA.
pub(super) fn find_first(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let attributes = call.cpu.cx() as u8;
    let result = read_path(call).and_then(|path| {
        state
            .process
            .active_mut()
            .files
            .find_first(&state.filesystem, &path, attributes)
    });
    return_search(call, state, result)
}

/// AH=4Fh: continue the search saved in the current DTA.
pub(super) fn find_next(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
) -> HookResult {
    let address = dta_address(state);
    let mut data = [0; FIND_DATA_BYTES];
    call.memory.read(address, &mut data).expect("real-mode DTA");
    let result = state
        .process
        .active()
        .files
        .find_next(&state.filesystem, &data);
    return_search(call, state, result)
}

fn return_search(
    call: &mut CpuContext<'_, DosServices>,
    state: &mut DosState,
    result: Result<[u8; FIND_DATA_BYTES], DosError>,
) -> HookResult {
    let result = result.map(|data| {
        let address = dta_address(state);
        call.memory.write(address, &data).expect("real-mode DTA");
    });
    return_result(call, result)
}

fn dta_address(state: &DosState) -> u32 {
    let process = state.process.active();
    u32::from(process.dta_segment) * 16 + u32::from(process.dta_offset)
}
