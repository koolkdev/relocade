use super::Services;
use crate::{memory, DosError};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, MemoryView,
};

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
    memory::allocate(
        memory,
        paragraphs,
        services.process.psp_segment(),
        services.dos.allocation_strategy.get(),
    )
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

/// AH=58h controls the session-wide memory allocation strategy.
#[interrupt(context(call), args(function = al))]
pub(super) async fn strategy(call: &mut CpuContext<'_>, services: &Services<'_>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => get_strategy(services),
            0x01 => set_strategy(services),
            _ => invalid_function,
        }
    );
}

#[interrupt(returns { strategy => { ax = strategy, cf = false } })]
fn get_strategy(services: &Services<'_>) -> u16 {
    services.dos.allocation_strategy.get().code()
}

#[interrupt(args(code = bx), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
fn set_strategy(services: &Services<'_>, code: u16) -> Result<(), DosError> {
    let strategy = memory::AllocationStrategy::from_code(code)?;
    services.dos.allocation_strategy.set(strategy);
    Ok(())
}

#[interrupt(returns { error => crate::interrupts::return_error(error) })]
fn invalid_function() -> DosError {
    DosError::InvalidFunction
}
