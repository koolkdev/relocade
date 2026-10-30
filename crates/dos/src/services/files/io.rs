//! DOS handle transfers through a resolved file or device record.

use crate::{
    error::DosError,
    files::{Device, Handle},
    interrupt::{return_result, DosContext},
    services::{
        buffers::transfer_count,
        console::{read_line, Console},
    },
};
use relocade_machine::HookResult;
use relocade_x86::Segment;

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

/// AH=3Fh: retain the source and DS:DX destination throughout a pending read.
pub(in crate::services) async fn read(call: &mut DosContext) -> HookResult {
    let cpu = &call.execution.cpu;
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let source = call.process.files().readable_handle(cpu.bx());
    let source = match source {
        Ok(source) => source,
        Err(error) => return return_result(call, Err(error)),
    };
    let console = call.console();
    let result = read_bytes(&console, &source, count).await;
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => return return_result(call, Err(error)),
    };
    if !bytes.is_empty() {
        call.execution
            .memory
            .view()
            .write_segment(&segment, u32::from(offset), &bytes)
            .expect("real-mode read buffer");
    }
    call.execution.cpu.set_ax(bytes.len() as u16);
    return_result(call, Ok(()))
}

async fn read_bytes(
    console: &Console<'_>,
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
                    let mut bytes = read_line(
                        &console.stdio.input,
                        source,
                        CONSOLE_LINE_CAPACITY,
                        &console.stream(stream),
                    )
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

/// AH=40h: write CX bytes to BX; CX=0 truncates or extends a file at its cursor.
pub(in crate::services) fn write(call: &mut DosContext) -> HookResult {
    let cpu = &call.execution.cpu;
    let handle = cpu.bx();
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let mut bytes = vec![0; count];
    if count != 0 {
        call.execution
            .memory
            .view()
            .read_segment(&segment, u32::from(offset), &mut bytes)
            .expect("real-mode write buffer");
    }
    let destination = call.process.files().writable_handle(handle);
    let result = destination
        .and_then(|destination| write_bytes(call, &destination, &bytes))
        .map(|count| call.execution.cpu.set_ax(count as u16));
    return_result(call, result)
}

fn write_bytes(
    call: &mut DosContext,
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
                    call.console().write(stream, bytes);
                    return Ok(bytes.len());
                }
                // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
                let count = bytes
                    .iter()
                    .position(|&byte| byte == 0x1a)
                    .unwrap_or(bytes.len());
                for &byte in &bytes[..count] {
                    call.console().stream(stream).write_cooked(byte);
                }
                Ok(count)
            }
        },
    }
}
