use super::files::read_path;
use crate::{error::DosError, files::FIND_DATA_BYTES, interrupt::return_result, DosState};
use relocade_runtime::{real_mode_interrupt::return_from_interrupt, HostCall};
use relocade_x86::{Segment, StoredSegment};
use std::ops::ControlFlow;

/// AH=1Ah: select the disk transfer area at DS:DX, preserving flags.
pub(super) fn set_dta(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let segment = call.cpu().segment(Segment::Ds).selector;
    let offset = call.cpu().dx();
    call.state_mut().dta_segment = segment;
    call.state_mut().dta_offset = offset;
    return_from_interrupt(call)
}

/// AH=2Fh: return the current DTA in ES:BX, preserving flags.
pub(super) fn get_dta(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let segment = call.state().dta_segment;
    let offset = call.state().dta_offset;
    call.cpu()
        .set_segment(Segment::Es, StoredSegment::real_mode(Segment::Es, segment))
        .expect("real-mode DTA segment");
    call.cpu().set_bx(offset);
    return_from_interrupt(call)
}

/// AH=4Eh: find DS:DX's 8.3 pattern with CX's attributes; fill the current DTA.
pub(super) fn find_first(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let attributes = call.cpu().cx() as u8;
    let result =
        read_path(call).and_then(|path| call.state_mut().files.find_first(&path, attributes));
    return_search(call, result)
}

/// AH=4Fh: continue the search saved in the current DTA.
pub(super) fn find_next(call: &mut HostCall<'_, DosState>) -> ControlFlow<()> {
    let address = dta_address(call);
    let mut data = [0; FIND_DATA_BYTES];
    call.memory()
        .read(address, &mut data)
        .expect("real-mode DTA");
    let result = call.state().files.find_next(&data);
    return_search(call, result)
}

fn return_search(
    call: &mut HostCall<'_, DosState>,
    result: Result<[u8; FIND_DATA_BYTES], DosError>,
) -> ControlFlow<()> {
    let result = result.map(|data| {
        let address = dta_address(call);
        call.memory().write(address, &data).expect("real-mode DTA");
    });
    return_result(call, result)
}

fn dta_address(call: &HostCall<'_, DosState>) -> u32 {
    u32::from(call.state().dta_segment) * 16 + u32::from(call.state().dta_offset)
}
