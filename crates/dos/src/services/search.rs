use super::{files::read_path, Services};
use crate::{files::FIND_DATA_BYTES, DosError};
use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};

#[interrupt(args(address = ptr(ds, dx)))]
pub(super) fn set_dta(services: &Services<'_>, address: FarPointer) {
    *services.process.dta() = address;
}

#[interrupt(returns { address => { ptr(es, bx) = address } })]
pub(super) fn get_dta(services: &Services<'_>) -> FarPointer {
    *services.process.dta()
}

#[interrupt(context(memory), args(pattern = ptr(ds, dx), attributes = cx), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn find_first(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
    pattern: FarPointer,
    attributes: u16,
) -> Result<(), DosError> {
    let pattern = read_path(memory, pattern)?;
    let data = services.process.files().find_first(
        &services.dos.filesystem(),
        &pattern,
        attributes as u8,
    )?;
    write_result(memory, services, &data);
    Ok(())
}

#[interrupt(context(memory), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn find_next(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
) -> Result<(), DosError> {
    let address = services.process.dta().address();
    let mut data = [0; FIND_DATA_BYTES];
    memory.read(address, &mut data).expect("real-mode DTA");
    let data = services
        .process
        .files()
        .find_next(&services.dos.filesystem(), &data)?;
    write_result(memory, services, &data);
    Ok(())
}

fn write_result(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
    data: &[u8; FIND_DATA_BYTES],
) {
    let address = services.process.dta().address();
    memory.write(address, data).expect("real-mode DTA");
}
