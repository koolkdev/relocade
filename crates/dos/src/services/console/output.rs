//! DOS output routing, cooked character handling and the CON text screen.

use crate::{
    error::DosError,
    files::{Device, HandleType},
    stdio::OutputStream,
    DosState,
};
use relocade_runtime::HostCall;

#[derive(Clone, Copy)]
pub(super) enum CharacterOutput {
    Stdout,
    Console(OutputStream),
}

pub(super) fn write_console(call: &mut HostCall<'_, DosState>, stream: OutputStream, bytes: &[u8]) {
    call.state().files.stdio.output(stream).extend(bytes);
    relocade_bios::write_text(&mut call.memory(), bytes);
}

/// DOS cooked output expands tabs, tracks its column and ignores write errors.
/// Returns the last output byte (a space for a tab).
pub(super) fn write_cooked_character(
    call: &mut HostCall<'_, DosState>,
    output: CharacterOutput,
    byte: u8,
) -> u8 {
    let column = &mut call.state_mut().files.stdio.output_column;
    let (byte, count) = if byte == b'\t' {
        (b' ', 8 - (*column & 7))
    } else {
        (byte, 1)
    };
    match byte {
        b'\r' => *column = 0,
        b'\x08' => *column = column.wrapping_sub(1),
        b' '..=b'~' | 0x80..=0xff => *column = column.wrapping_add(count),
        _ => {}
    }
    let bytes = [byte; 8];
    let bytes = &bytes[..usize::from(count)];
    match output {
        CharacterOutput::Stdout => {
            let _ = write_characters(call, 1, bytes);
        }
        CharacterOutput::Console(stream) => write_console(call, stream, bytes),
    }
    byte
}

/// Character output preserves console EOF; only AH=40h handle writes clear it.
pub(super) fn write_characters(
    call: &mut HostCall<'_, DosState>,
    handle: u16,
    bytes: &[u8],
) -> Result<(), DosError> {
    match call.state().files.writable_handle_type(handle)? {
        HandleType::Device(Device::Console(stream)) => write_console(call, stream, bytes),
        HandleType::File => {
            if !bytes.is_empty() {
                call.state_mut().files.write(handle, bytes)?;
            }
        }
        HandleType::Device(Device::Null) => {}
        HandleType::Device(Device::Auxiliary | Device::Printer) => {
            return Err(DosError::AccessDenied);
        }
    }
    Ok(())
}

/// AH=40h console writes share character output but honor raw mode and clear EOF.
pub(in crate::services) fn write_handle(
    call: &mut HostCall<'_, DosState>,
    handle: u16,
    bytes: &[u8],
) -> Result<usize, DosError> {
    let HandleType::Device(Device::Console(stream)) =
        call.state().files.writable_handle_type(handle)?
    else {
        return call.state_mut().files.write(handle, bytes);
    };
    // Even a zero-byte handle write clears EOF on this record and its duplicates.
    call.state().files.set_console_eof(handle, false);
    if call.state().files.console_raw(handle) {
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
