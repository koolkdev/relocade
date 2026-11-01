//! File and device transfers over resolved handles and console resources.

use crate::{error::DosError, files::Handle, services::console::read_line, Console};

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

pub(super) async fn read(
    console: Console<'_>,
    source: &Handle,
    count: usize,
) -> Result<Vec<u8>, DosError> {
    let handle = match source {
        Handle::File(file) => {
            let mut bytes = vec![0; count];
            let count = file.read(&mut bytes)?;
            bytes.truncate(count);
            return Ok(bytes);
        }
        Handle::Device(handle) => handle,
    };
    if handle.device.is_null() {
        if count != 0 {
            handle.set_eof(true);
        }
        return Ok(Vec::new());
    }
    if !handle.device.is_console() {
        return Err(DosError::AccessDenied);
    }
    if handle.eof() {
        return Ok(Vec::new());
    }
    if handle.raw() {
        let mut bytes = Vec::with_capacity(count);
        while bytes.len() < count {
            bytes.push(handle.device.read_character().await?);
        }
        return Ok(bytes);
    }
    if count != 0 && !console.stdio.has_buffered_line() {
        let mut bytes = read_line(
            source,
            CONSOLE_LINE_CAPACITY,
            console.device_output(handle.clone()),
        )
        .await?;
        let eof = bytes.first() == Some(&0x1a);
        handle.set_eof(eof);
        console.device_output(handle.clone()).write(b"\n")?;
        if !eof {
            bytes.push(b'\n');
            console.stdio.buffer_line(bytes);
        }
    }
    Ok(console.stdio.take_line_bytes(count))
}

pub(super) fn write(
    console: Console<'_>,
    destination: &Handle,
    bytes: &[u8],
) -> Result<usize, DosError> {
    let handle = match destination {
        Handle::File(file) => return file.write(bytes),
        Handle::Device(handle) => handle,
    };
    let output = console.device_output(handle.clone());
    if handle.device.is_null() || handle.raw() {
        output.write(bytes)?;
        handle.set_eof(false);
        return Ok(bytes.len());
    }
    if !handle.device.is_console() {
        return Err(DosError::AccessDenied);
    }
    handle.set_eof(false);
    // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
    let count = bytes
        .iter()
        .position(|&byte| byte == 0x1a)
        .unwrap_or(bytes.len());
    for &byte in &bytes[..count] {
        output.write_cooked(byte);
    }
    Ok(count)
}
