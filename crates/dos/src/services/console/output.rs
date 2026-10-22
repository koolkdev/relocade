//! DOS output routing, cooked character handling and the CON text screen.

use crate::{
    error::DosError,
    files::{Device, HandleType},
    state::DosState,
    stdio::OutputStream,
};
use relocade_machine::CpuContext;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub(super) enum CharacterOutput {
    Stdout,
    Console(OutputStream),
}

pub(super) fn write_console(
    call: &mut CpuContext<'_>,
    state: &Rc<DosState>,
    stream: OutputStream,
    bytes: &[u8],
) {
    state.files.borrow_mut().stdio.output(stream).extend(bytes);
    relocade_bios::write_text(&mut call.memory(), bytes);
}

/// DOS cooked output expands tabs, tracks its column and ignores write errors.
/// Returns the last output byte (a space for a tab).
pub(super) fn write_cooked_character(
    call: &mut CpuContext<'_>,
    state: &Rc<DosState>,
    output: CharacterOutput,
    byte: u8,
) -> u8 {
    let (byte, count) = {
        let mut files = state.files.borrow_mut();
        let column = &mut files.stdio.output_column;
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
        (byte, count)
    };
    let bytes = [byte; 8];
    let bytes = &bytes[..usize::from(count)];
    match output {
        CharacterOutput::Stdout => {
            let _ = write_characters(call, state, 1, bytes);
        }
        CharacterOutput::Console(stream) => write_console(call, state, stream, bytes),
    }
    byte
}

/// Character output preserves console EOF; only AH=40h handle writes clear it.
pub(super) fn write_characters(
    call: &mut CpuContext<'_>,
    state: &Rc<DosState>,
    handle: u16,
    bytes: &[u8],
) -> Result<(), DosError> {
    let kind = state.files.borrow().writable_handle_type(handle)?;
    match kind {
        HandleType::Device(Device::Console(stream)) => write_console(call, state, stream, bytes),
        HandleType::File => {
            if !bytes.is_empty() {
                state.files.borrow_mut().write(handle, bytes)?;
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
    call: &mut CpuContext<'_>,
    state: &Rc<DosState>,
    handle: u16,
    bytes: &[u8],
) -> Result<usize, DosError> {
    let kind = state.files.borrow().writable_handle_type(handle)?;
    let HandleType::Device(Device::Console(stream)) = kind else {
        return state.files.borrow_mut().write(handle, bytes);
    };
    // Even a zero-byte handle write clears EOF on this record and its duplicates.
    state.files.borrow_mut().set_console_eof(handle, false);
    if state.files.borrow().console_raw(handle) {
        write_console(call, state, stream, bytes);
        return Ok(bytes.len());
    }
    // Cooked writes stop before Ctrl-Z and count source bytes, including tabs.
    let count = bytes
        .iter()
        .position(|&byte| byte == 0x1a)
        .unwrap_or(bytes.len());
    for &byte in &bytes[..count] {
        write_cooked_character(call, state, CharacterOutput::Console(stream), byte);
    }
    Ok(count)
}
