use super::{files::read_path, Services};
use crate::DosError;
use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};

#[interrupt(args(number = dl), returns { count => { al = count } })]
pub(super) fn select_drive(services: &Services<'_>, number: u8) -> u8 {
    services.dos.filesystem().select_drive(number)
}

#[interrupt(returns { number => { al = number } })]
pub(super) fn get_drive(services: &Services<'_>) -> u8 {
    services.dos.filesystem().current_drive()
}

#[interrupt(context(memory), args(path = ptr(ds, dx)), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(super) fn create_directory(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
) -> Result<(), DosError> {
    let path = read_path(memory, path)?;
    services.dos.filesystem().create_directory(&path)
}

#[interrupt(context(memory), args(path = ptr(ds, dx)), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(super) fn remove_directory(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
) -> Result<(), DosError> {
    let path = read_path(memory, path)?;
    services.dos.filesystem().remove_directory(&path)
}

#[interrupt(context(memory), args(path = ptr(ds, dx)), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(super) fn change_directory(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
) -> Result<(), DosError> {
    let path = read_path(memory, path)?;
    services.dos.filesystem().change_directory(&path)
}

/// AH=47h writes a directory without a drive prefix or leading slash.
#[interrupt(context(memory), args(drive = dl, destination = ptr(ds, si)), returns {
    Ok(()) => { ax = 0x0100u16, cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(super) fn get_directory(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
    drive: u8,
    destination: FarPointer,
) -> Result<(), DosError> {
    let mut bytes = services
        .dos
        .filesystem()
        .current_directory(drive)?
        .replace('/', "\\")
        .into_bytes();
    bytes.push(0);
    memory
        .write_wrapping(destination, &bytes)
        .expect("current directory buffer");
    Ok(())
}
