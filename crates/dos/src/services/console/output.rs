//! DOS output routing, cooked character handling and the CON text screen.

use crate::{
    error::DosError,
    files::{Device, Handle},
    interrupt::DosContext,
    stdio::OutputStream,
};

#[derive(Clone, Copy)]
pub(in crate::services) enum CharacterOutput {
    Stdout,
    Console(OutputStream),
}

pub(in crate::services) fn write_console(
    call: &mut DosContext,
    stream: OutputStream,
    bytes: &[u8],
) {
    call.dos.stdio().output(stream).extend(bytes);
    relocade_bios::write_text(&mut call.execution.memory(), &call.execution.io, bytes);
}

/// DOS cooked output expands tabs, tracks its column and ignores write errors.
/// Returns the last output byte (a space for a tab).
pub(in crate::services) fn write_cooked_character(
    call: &mut DosContext,
    output: CharacterOutput,
    byte: u8,
) -> u8 {
    let (byte, count) = call.dos.stdio().cook_character(byte);
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
    call: &mut DosContext,
    handle: u16,
    bytes: &[u8],
) -> Result<(), DosError> {
    let handle = call.process.files().writable_handle(handle)?;
    match handle {
        Handle::File(file) => {
            if !bytes.is_empty() {
                file.write(bytes)?;
            }
        }
        Handle::Device(device) => match device.kind() {
            Device::Console(stream) => write_console(call, stream, bytes),
            Device::Null => {}
            Device::Auxiliary | Device::Printer => return Err(DosError::AccessDenied),
        },
    }
    Ok(())
}
