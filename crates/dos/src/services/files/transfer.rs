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
    if handle.eof() || count == 0 {
        return Ok(Vec::new());
    }
    if handle.raw() || !handle.device.is_console() {
        let mut bytes = vec![0; count];
        let count = handle.device.read(&mut bytes).await?;
        assert!(count <= bytes.len(), "driver read exceeded its buffer");
        bytes.truncate(count);
        return Ok(bytes);
    }
    if !console.stdio.has_buffered_line() {
        let mut bytes = read_line(
            source,
            CONSOLE_LINE_CAPACITY,
            console.device_output(handle.clone()),
        )
        .await?;
        let eof = bytes.first() == Some(&0x1a);
        handle.set_eof(eof);
        console
            .device_output(handle.clone())
            .write_all(b"\n")
            .await?;
        if !eof {
            bytes.push(b'\n');
            console.stdio.buffer_line(bytes);
        }
    }
    Ok(console.stdio.take_line_bytes(count))
}

pub(super) async fn write(
    console: Console<'_>,
    destination: &Handle,
    bytes: &[u8],
) -> Result<usize, DosError> {
    let handle = match destination {
        Handle::File(file) => return file.write(bytes),
        Handle::Device(handle) => handle,
    };
    let output = console.device_output(handle.clone());
    if !handle.device.is_console() || handle.raw() {
        let count = output.write(bytes).await?;
        handle.set_eof(false);
        return Ok(count);
    }
    handle.set_eof(false);
    // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
    let count = bytes
        .iter()
        .position(|&byte| byte == 0x1a)
        .unwrap_or(bytes.len());
    for &byte in &bytes[..count] {
        output.write_cooked(byte).await?;
    }
    Ok(count)
}
