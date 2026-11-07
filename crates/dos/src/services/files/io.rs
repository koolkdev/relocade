//! DOS handle transfers retain arguments and buffers across ordinary I/O waits.

use crate::{
    services::{control::Request, io},
    DosError,
};
use relocade_machine::{real_mode_interrupt::interrupt, CpuContext, FarPointer};
use std::ops::ControlFlow;

/// AH=3Fh/40h stop at the segment end instead of wrapping a file transfer buffer.
fn transfer_count(buffer: FarPointer, count: u16) -> usize {
    usize::from(count).min(0x10000 - usize::from(buffer.offset))
}

#[interrupt(context(call), args(handle = bx, buffer = ptr(ds, dx), count = cx), returns {
    Ok(count) => { ax = count, cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(in crate::services) async fn read(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    handle: u16,
    buffer: FarPointer,
    count: u16,
) -> Result<u16, DosError> {
    let count = transfer_count(buffer, count);
    let services = request.services;
    loop {
        let source = services.process.files().readable_handle(handle)?;
        let ControlFlow::Continue(bytes) = io::read_handle(call, request, &source, count).await?
        else {
            continue;
        };
        call.memory
            .write_wrapping(buffer, &bytes)
            .expect("real-mode read buffer");
        return Ok(bytes.len() as u16);
    }
}

/// A zero-length AH=40h write truncates or extends a file at its current cursor.
#[interrupt(context(call), args(handle = bx, buffer = ptr(ds, dx), count = cx), returns {
    Ok(count) => { ax = count, cf = false },
    Err(error) => crate::interrupts::return_error(error),
})]
pub(in crate::services) async fn write(
    call: &mut CpuContext<'_>,
    request: &mut Request<'_>,
    handle: u16,
    buffer: FarPointer,
    count: u16,
) -> Result<u16, DosError> {
    let count = transfer_count(buffer, count);
    let services = request.services;
    loop {
        // INT23 may change guest memory or redirection before restarting the call.
        let mut bytes = vec![0; count];
        call.memory
            .read_wrapping(buffer, &mut bytes)
            .expect("real-mode write buffer");
        let destination = services.process.files().writable_handle(handle)?;
        let ControlFlow::Continue(count) =
            io::write_handle(call, request, &destination, &bytes).await?
        else {
            continue;
        };
        return Ok(count as u16);
    }
}
