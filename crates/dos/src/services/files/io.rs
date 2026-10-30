//! DOS handle transfers through a resolved file or device record.

use crate::{
    error::DosError,
    files::{Device, Handle},
    interrupt::{return_result, DosContext},
    services::{
        buffers::transfer_count,
        console::{read_line, write_console, write_cooked_character, CharacterOutput},
    },
};
use relocade_machine::HookResult;
use relocade_x86::Segment;

// DOS's console handle buffer allows 127 input bytes plus CR; reads also return LF.
const CONSOLE_LINE_CAPACITY: u8 = 128;

/// AH=3Fh: retain the source and DS:DX destination throughout a pending read.
pub(in crate::services) async fn read(call: &mut DosContext) -> HookResult {
    let cpu = &call.execution.cpu;
    let handle = cpu.bx();
    let offset = cpu.dx();
    let count = transfer_count(offset, cpu.cx());
    let segment = cpu.segment(Segment::Ds);
    let source = call.process.files().readable_handle(handle);
    let source = match source {
        Ok(source) => source,
        Err(error) => return return_result(call, Err(error)),
    };
    let bytes = match read_bytes(call, &source, count).await {
        Ok(bytes) => bytes,
        Err(error) => return return_result(call, Err(error)),
    };
    if !bytes.is_empty() {
        call.execution
            .memory()
            .write_segment(&segment, u32::from(offset), &bytes)
            .expect("real-mode read buffer");
    }
    call.execution.cpu.set_ax(bytes.len() as u16);
    return_result(call, Ok(()))
}

async fn read_bytes(
    call: &mut DosContext,
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
                        bytes.push(call.dos.stdio().input.read_character().await);
                    }
                    return Ok(bytes);
                }
                if count != 0 && !call.dos.stdio().has_buffered_line() {
                    let mut bytes = read_line(
                        call,
                        source,
                        CONSOLE_LINE_CAPACITY,
                        CharacterOutput::Console(stream),
                    )
                    .await?;
                    let eof = bytes.first() == Some(&0x1a);
                    device.set_eof(eof);
                    write_console(call, stream, b"\n");
                    if !eof {
                        bytes.push(b'\n');
                        call.dos.stdio().buffer_line(bytes);
                    }
                }
                Ok(call.dos.stdio().take_line_bytes(count))
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
            .memory()
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
                    write_console(call, stream, bytes);
                    return Ok(bytes.len());
                }
                // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
                let count = bytes
                    .iter()
                    .position(|&byte| byte == 0x1a)
                    .unwrap_or(bytes.len());
                for &byte in &bytes[..count] {
                    write_cooked_character(call, CharacterOutput::Console(stream), byte);
                }
                Ok(count)
            }
        },
    }
}
