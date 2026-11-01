//! Route resolved handles to file, raw-device or cooked-console transfers.

use crate::{error::DosError, files::Handle, services::console::cooked, stdio::Stdio};

pub(super) async fn read(
    stdio: &Stdio,
    source: &Handle,
    count: usize,
) -> Result<Vec<u8>, DosError> {
    if let Handle::Device(handle) = source {
        if handle.device.is_null() {
            if count != 0 {
                handle.set_eof(true);
            }
            return Ok(Vec::new());
        }
        if handle.eof() || count == 0 {
            return Ok(Vec::new());
        }
        if handle.device.is_console() && !handle.raw() {
            return cooked::read(stdio, handle, count).await;
        }
    }
    let mut bytes = vec![0; count];
    let count = source.read(&mut bytes).await?;
    bytes.truncate(count);
    Ok(bytes)
}

pub(super) async fn write(
    stdio: &Stdio,
    destination: &Handle,
    bytes: &[u8],
) -> Result<usize, DosError> {
    if let Handle::Device(handle) = destination {
        if handle.device.is_console() && !handle.raw() {
            return cooked::write(stdio, handle, bytes).await;
        }
    }
    let count = destination.write(bytes).await?;
    if let Handle::Device(handle) = destination {
        handle.set_eof(false);
    }
    Ok(count)
}
