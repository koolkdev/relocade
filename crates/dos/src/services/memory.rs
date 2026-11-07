use super::Services;
use crate::{memory, DosError};
use relocade_machine::{real_mode_interrupt::interrupt, MemoryView};

/// AH=48h allocates paragraphs and returns their segment.
#[interrupt(context(memory), args(paragraphs = bx), returns {
    Ok(segment) => { ax = segment, cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(super) fn allocate(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
    paragraphs: u16,
) -> Result<u16, DosError> {
    memory::allocate(memory, paragraphs, services.process.psp_segment())
}

/// AH=49h frees the block whose data begins at ES:0000.
#[interrupt(context(memory), args(segment = es), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(super) fn free(memory: &mut MemoryView<'_>, segment: u16) -> Result<(), DosError> {
    memory::free(memory, segment)
}

/// AH=4Ah resizes the block at ES:0000 to BX paragraphs.
#[interrupt(context(memory), args(segment = es, paragraphs = bx), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(super) fn resize(
    memory: &mut MemoryView<'_>,
    segment: u16,
    paragraphs: u16,
) -> Result<(), DosError> {
    memory::resize(memory, segment, paragraphs)
}
