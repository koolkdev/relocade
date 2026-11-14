//! File attributes, timestamps and pathname operations.

use super::{read_path, Services};
use crate::{files::FileTimestamp, DosError};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, FarPointer, MemoryView,
};

#[interrupt(context(call), args(function = al))]
pub(in crate::services) async fn attributes(
    call: &mut CpuContext<'_>,
    services: &Services<'_>,
    function: u8,
) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => get_attributes(services),
            0x01 => set_attributes(services),
            _ => unsupported,
        }
    );
}

#[interrupt(context(memory), args(path = ptr(ds, dx)), returns {
    Ok(attributes) => { cx = attributes, cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
fn get_attributes(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
) -> Result<u16, DosError> {
    let path = read_path(memory, path)?;
    services.dos.filesystem().attributes(&path)
}

#[interrupt(context(memory), args(path = ptr(ds, dx), attributes = cx), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
fn set_attributes(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
    attributes: u16,
) -> Result<(), DosError> {
    let path = read_path(memory, path)?;
    services.dos.filesystem().set_attributes(&path, attributes)
}

#[interrupt(returns { error => crate::interrupts::return_error(error) })]
fn unsupported() -> DosError {
    DosError::InvalidFunction
}

#[interrupt(context(call), args(function = al))]
pub(in crate::services) async fn timestamp(
    call: &mut CpuContext<'_>,
    services: &Services<'_>,
    function: u8,
) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => get_timestamp(services),
            0x01 => set_timestamp(services),
            _ => unsupported,
        }
    );
}

#[interrupt(args(handle = bx), returns {
    Ok(timestamp) => { cx = timestamp.time, dx = timestamp.date, cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
fn get_timestamp(services: &Services<'_>, handle: u16) -> Result<FileTimestamp, DosError> {
    services.process.files().timestamp(handle)
}

#[interrupt(args(handle = bx, time = cx, date = dx), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
fn set_timestamp(
    services: &Services<'_>,
    handle: u16,
    time: u16,
    date: u16,
) -> Result<(), DosError> {
    services
        .process
        .files()
        .set_timestamp(handle, FileTimestamp { time, date })
}

#[interrupt(context(memory), args(path = ptr(ds, dx)), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(in crate::services) fn delete(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
) -> Result<(), DosError> {
    let path = read_path(memory, path)?;
    services
        .process
        .files()
        .delete(&mut services.dos.filesystem(), &path)
}

#[interrupt(context(memory), args(source = ptr(ds, dx), destination = ptr(es, di)), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(in crate::services) fn rename(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    source: FarPointer,
    destination: FarPointer,
) -> Result<(), DosError> {
    let from = read_path(memory, source)?;
    let to = read_path(memory, destination)?;
    services
        .process
        .files()
        .rename(&mut services.dos.filesystem(), &from, &to)
}
