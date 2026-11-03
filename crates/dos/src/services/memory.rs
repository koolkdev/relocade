use super::Services;
use crate::{interrupt::return_result, memory};
use relocade_machine::real_mode_interrupt::interrupt;
use relocade_machine::CpuContext;
use relocade_x86::Segment;

/// AH=48h: allocate BX paragraphs; return the segment in AX.
#[interrupt(context(call))]
pub(super) fn allocate(call: &mut CpuContext<'_>, services: &Services<'_>) {
    let paragraphs = call.cpu.bx();
    let result = memory::allocate(&mut call.memory, paragraphs, services.process.psp_segment());
    let result = result.map(|segment| call.cpu.set_ax(segment));
    return_result(call, result)
}

/// AH=49h: free the block whose data begins at ES:0000.
#[interrupt(context(call))]
pub(super) fn free(call: &mut CpuContext<'_>) {
    let segment = call.cpu.segment(Segment::Es).selector;
    let result = memory::free(&mut call.memory, segment);
    return_result(call, result)
}

/// AH=4Ah: resize the block at ES:0000 to BX paragraphs.
#[interrupt(context(call))]
pub(super) fn resize(call: &mut CpuContext<'_>) {
    let segment = call.cpu.segment(Segment::Es).selector;
    let paragraphs = call.cpu.bx();
    let result = memory::resize(&mut call.memory, segment, paragraphs);
    return_result(call, result)
}
