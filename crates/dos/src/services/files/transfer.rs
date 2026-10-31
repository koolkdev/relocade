//! File and device transfers over resolved handles and console resources.

use crate::{
    error::DosError,
    files::{Device, Handle},
    services::console::{read_line, Console},
};

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

pub(super) async fn read(
    console: Console<'_>,
    source: &Handle,
    count: usize,
) -> Result<Vec<u8>, DosError> {
    match source {
        Handle::File(file) => {
            let mut bytes = vec![0; count];
            let count = file.read(&mut bytes)?;
            bytes.truncate(count);
            Ok(bytes)
        }
        Handle::Device(device) => match device.kind() {
            Device::Null => {
                if count != 0 {
                    device.set_eof(true);
                }
                Ok(Vec::new())
            }
            Device::Auxiliary | Device::Printer => Err(DosError::AccessDenied),
            Device::Console(stream) => {
                if device.eof() {
                    return Ok(Vec::new());
                }
                if device.raw() {
                    let mut bytes = Vec::with_capacity(count);
                    while bytes.len() < count {
                        bytes.push(console.stdio.input.read_character().await);
                    }
                    return Ok(bytes);
                }
                if count != 0 && !console.stdio.has_buffered_line() {
                    let output = console.stream(stream);
                    let mut bytes =
                        read_line(&console.stdio.input, source, CONSOLE_LINE_CAPACITY, output)
                            .await?;
                    let eof = bytes.first() == Some(&0x1a);
                    device.set_eof(eof);
                    console.write(stream, b"\n");
                    if !eof {
                        bytes.push(b'\n');
                        console.stdio.buffer_line(bytes);
                    }
                }
                Ok(console.stdio.take_line_bytes(count))
            }
        },
    }
}

pub(super) fn write(
    console: Console<'_>,
    destination: &Handle,
    bytes: &[u8],
) -> Result<usize, DosError> {
    match destination {
        Handle::File(file) => file.write(bytes),
        Handle::Device(device) => match device.kind() {
            Device::Null => {
                // Even a zero-byte handle write clears EOF on this record and its duplicates.
                device.set_eof(false);
                Ok(bytes.len())
            }
            Device::Auxiliary | Device::Printer => Err(DosError::AccessDenied),
            Device::Console(stream) => {
                device.set_eof(false);
                if device.raw() {
                    console.write(stream, bytes);
                    return Ok(bytes.len());
                }
                // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
                let count = bytes
                    .iter()
                    .position(|&byte| byte == 0x1a)
                    .unwrap_or(bytes.len());
                let output = console.stream(stream);
                for &byte in &bytes[..count] {
                    output.write_cooked(byte);
                }
                Ok(count)
            }
        },
    }
}
