//! DOS handle transfers retain their buffers and handles across pending I/O.

use super::transfer;
use crate::{services::Services, DosError};
use relocade_machine::{real_mode_interrupt::interrupt, FarPointer, MemoryView};

/// AH=3Fh/40h stop at the segment end instead of wrapping a file transfer buffer.
fn transfer_count(buffer: FarPointer, count: u16) -> usize {
    usize::from(count).min(0x10000 - usize::from(buffer.offset))
}

#[interrupt(context(memory), args(handle = bx, buffer = ptr(ds, dx), count = cx), returns {
    Ok(count) => { ax = count, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(in crate::services) async fn read(
    memory: &mut MemoryView<'_>,
    services: &Services<'_>,
    handle: u16,
    buffer: FarPointer,
    count: u16,
) -> Result<u16, DosError> {
    let count = transfer_count(buffer, count);
    let source = services.process.files().readable_handle(handle)?;
    let bytes = transfer::read(services.dos.stdio(), &source, count).await?;
    memory
        .write_wrapping(buffer, &bytes)
        .expect("real-mode read buffer");
    Ok(bytes.len() as u16)
}

/// A zero-length AH=40h write truncates or extends a file at its current cursor.
#[interrupt(context(memory), args(handle = bx, buffer = ptr(ds, dx), count = cx), returns {
    Ok(count) => { ax = count, cf = false },
    Err(error) => crate::interrupt::return_error(error),
})]
pub(in crate::services) async fn write(
    memory: &MemoryView<'_>,
    services: &Services<'_>,
    handle: u16,
    buffer: FarPointer,
    count: u16,
) -> Result<u16, DosError> {
    let mut bytes = vec![0; transfer_count(buffer, count)];
    memory
        .read_wrapping(buffer, &mut bytes)
        .expect("real-mode write buffer");
    let destination = services.process.files().writable_handle(handle)?;
    let count = transfer::write(services.dos.stdio(), &destination, &bytes).await?;
    Ok(count as u16)
}
