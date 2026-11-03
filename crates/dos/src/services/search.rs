use super::{files::read_path, Services};
use crate::{error::DosError, files::FIND_DATA_BYTES, interrupt::return_result};
use relocade_machine::real_mode_interrupt::interrupt;
use relocade_machine::CpuContext;
use relocade_x86::{Segment, SegmentRegister};

/// AH=1Ah: select the disk transfer area at DS:DX, preserving flags.
#[interrupt(context(call))]
pub(super) fn set_dta(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let segment = call.cpu.segment(Segment::Ds).selector;
    let offset = call.cpu.dx();
    let mut dta = services.process.dta();
    dta.segment = segment;
    dta.offset = offset;
}

/// AH=2Fh: return the current DTA in ES:BX, preserving flags.
#[interrupt(context(call))]
pub(super) fn get_dta(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let dta = services.process.dta();
    call.cpu.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, dta.segment),
    );
    call.cpu.set_bx(dta.offset);
}

/// AH=4Eh: find DS:DX's 8.3 pattern with CX's attributes; fill the current DTA.
#[interrupt(context(call))]
pub(super) fn find_first(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let attributes = call.cpu.cx() as u8;
    let result = read_path(call).and_then(|path| {
        services
            .process
            .files()
            .find_first(&services.dos.filesystem(), &path, attributes)
    });
    return_search(call, services, result)
}

/// AH=4Fh: continue the search saved in the current DTA.
#[interrupt(context(call))]
pub(super) fn find_next(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let address = services.process.dta().address();
    let mut data = [0; FIND_DATA_BYTES];
    call.memory.read(address, &mut data).expect("real-mode DTA");
    let result = services
        .process
        .files()
        .find_next(&services.dos.filesystem(), &data);
    return_search(call, services, result)
}

fn return_search(
    call: &mut CpuContext<'_>,
    services: &Services<'_>,
    result: Result<[u8; FIND_DATA_BYTES], DosError>,
) {
    let result = result.map(|data| {
        let address = services.process.dta().address();
        call.memory.write(address, &data).expect("real-mode DTA");
    });
    return_result(call, result)
}
