//! DOS file handles. Paths and transfer buffers use captured real-mode pointers.

use super::Services;
use crate::{files::CreateMode, path::MAX_PATH_BYTES, DosError};
use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};

pub(super) mod io;
pub(super) mod metadata;

#[interrupt(returns { error => crate::interrupt::return_error(error) })]
pub(super) fn long_filename() -> DosError {
    DosError::LongFilenameNotSupported
}

/// AH=3Ch creates or truncates the path; AH=5Bh rejects an existing file.
#[interrupt(context(memory), args(path = ptr(ds, dx), attributes = cx), returns {
    Ok(handle) => { ax = handle, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn create_or_truncate(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
    attributes: u16,
) -> Result<u16, DosError> {
    create(
        memory,
        services,
        path,
        attributes,
        CreateMode::TruncateExisting,
    )
}

#[interrupt(context(memory), args(path = ptr(ds, dx), attributes = cx), returns {
    Ok(handle) => { ax = handle, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn create_new(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
    attributes: u16,
) -> Result<u16, DosError> {
    create(memory, services, path, attributes, CreateMode::FailIfExists)
}

fn create(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
    attributes: u16,
    mode: CreateMode,
) -> Result<u16, DosError> {
    let path = read_path(memory, path)?;
    services
        .process
        .files()
        .create(&mut services.dos.filesystem(), &path, attributes, mode)
}

/// AH=5Ah creates a temporary file and writes its path back into the input buffer.
#[interrupt(context(memory), args(directory = ptr(ds, dx), attributes = cx), returns {
    Ok(handle) => { ax = handle, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn create_temporary(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
    directory: FarPointer,
    attributes: u16,
) -> Result<u16, DosError> {
    let mut path = read_path(memory, directory)?;
    let handle = services.process.files().create_temporary(
        &mut services.dos.filesystem(),
        &mut path,
        attributes,
    )?;
    path.push(0);
    memory
        .write_wrapping(directory, &path)
        .expect("temporary file path");
    Ok(handle)
}

#[interrupt(context(memory), args(path = ptr(ds, dx), mode = al), returns {
    Ok(handle) => { ax = handle, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn open(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    path: FarPointer,
    mode: u8,
) -> Result<u16, DosError> {
    let path = read_path(memory, path)?;
    services
        .process
        .files()
        .open(&services.dos.filesystem(), &path, mode)
}

#[interrupt(args(handle = bx), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn close(services: &Services<'_>, handle: u16) -> Result<(), DosError> {
    services
        .process
        .files()
        .close(&mut services.dos.filesystem(), handle)
}

/// Both commit entry points return AH=68h without closing or seeking the handle.
#[interrupt(args(handle = bx), returns {
    Ok(()) => { ah = 0x68u8, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn commit(services: &Services<'_>, handle: u16) -> Result<(), DosError> {
    services
        .process
        .files()
        .commit(&mut services.dos.filesystem(), handle)
}

#[interrupt(args(handle = bx), returns {
    Ok(()) => { ah = 0x68u8, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn commit_alias(services: &Services<'_>, handle: u16) -> Result<(), DosError> {
    commit(services, handle)
}

#[interrupt(args(source = bx), returns {
    Ok(handle) => { ax = handle, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn duplicate(services: &Services<'_>, source: u16) -> Result<u16, DosError> {
    services.process.files().duplicate(source)
}

#[interrupt(args(source = bx, destination = cx), returns {
    Ok(()) => { cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn force_duplicate(
    services: &Services<'_>,
    source: u16,
    destination: u16,
) -> Result<(), DosError> {
    services
        .process
        .files()
        .force_duplicate(&mut services.dos.filesystem(), source, destination)
}

#[interrupt(args(handle = bx, origin = al, offset = (cx, dx)), returns {
    Ok(position) => { (dx, ax) = position, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(super) fn seek(
    services: &Services<'_>,
    handle: u16,
    origin: u8,
    offset: u32,
) -> Result<u32, DosError> {
    services.process.files().seek(handle, origin, offset)
}

pub(super) fn read_path(memory: &MemoryView<'_>, source: FarPointer) -> Result<Vec<u8>, DosError> {
    let mut path = Vec::new();
    for index in 0..MAX_PATH_BYTES {
        let mut byte = [0];
        memory
            .read_wrapping(source.wrapping_add(index), &mut byte)
            .expect("real-mode path address");
        if byte[0] == 0 {
            return Ok(path);
        }
        path.push(byte[0]);
    }
    Err(DosError::PathNotFound)
}
