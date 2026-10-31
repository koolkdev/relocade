use super::files::read_path;
use crate::{
    error::DosError,
    files::FIND_DATA_BYTES,
    interrupt::{return_result, DosContext},
};
use relocade_machine::{real_mode_interrupt::return_from_interrupt, HookResult};
use relocade_x86::{Segment, SegmentRegister};

/// AH=1Ah: select the disk transfer area at DS:DX, preserving flags.
pub(super) fn set_dta(call: &mut DosContext<'_>) -> HookResult {
    let segment = call.cpu.segment(Segment::Ds).selector;
    let offset = call.cpu.dx();
    let mut dta = call.process.dta();
    dta.segment = segment;
    dta.offset = offset;
    return_from_interrupt(call.cpu)
}

/// AH=2Fh: return the current DTA in ES:BX, preserving flags.
pub(super) fn get_dta(call: &mut DosContext<'_>) -> HookResult {
    let dta = call.process.dta();
    call.cpu.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, dta.segment),
    );
    call.cpu.set_bx(dta.offset);
    return_from_interrupt(call.cpu)
}

/// AH=4Eh: find DS:DX's 8.3 pattern with CX's attributes; fill the current DTA.
pub(super) fn find_first(call: &mut DosContext<'_>) -> HookResult {
    let attributes = call.cpu.cx() as u8;
    let result = read_path(call).and_then(|path| {
        call.process
            .files()
            .find_first(&call.dos.filesystem(), &path, attributes)
    });
    return_search(call, result)
}

/// AH=4Fh: continue the search saved in the current DTA.
pub(super) fn find_next(call: &mut DosContext<'_>) -> HookResult {
    let address = call.process.dta().address();
    let mut data = [0; FIND_DATA_BYTES];
    call.memory.read(address, &mut data).expect("real-mode DTA");
    let result = call
        .process
        .files()
        .find_next(&call.dos.filesystem(), &data);
    return_search(call, result)
}

fn return_search(
    call: &mut DosContext<'_>,
    result: Result<[u8; FIND_DATA_BYTES], DosError>,
) -> HookResult {
    let result = result.map(|data| {
        let address = call.process.dta().address();
        call.memory.write(address, &data).expect("real-mode DTA");
    });
    return_result(call, result)
}
