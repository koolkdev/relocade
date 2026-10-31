//! DOS character reads, readiness, and flushing over a resolved handle.

use crate::{
    console::CharacterOutput,
    error::DosError,
    files::{Device, Handle},
};
use relocade_bios::Keyboard;
use std::future::pending;

/// DOS status queries ask the source, independently of cooked-line buffering and EOF flags.
pub(in crate::services) fn available(input: &Handle, keyboard: &Keyboard) -> bool {
    match input {
        Handle::File(file) => file.input_available(),
        Handle::Device(device) => match device.kind() {
            Device::Console(_) => keyboard.has_input(),
            Device::Null => true,
            Device::Auxiliary | Device::Printer => false,
        },
    }
}

/// Discard queued input from this source without consuming file data.
pub(super) fn flush(input: &Handle, keyboard: &Keyboard) {
    if matches!(input, Handle::Device(device) if matches!(device.kind(), Device::Console(_))) {
        keyboard.discard_queued();
    }
}

pub(super) fn try_read_character(
    input: &Handle,
    keyboard: &Keyboard,
) -> Result<Option<u8>, DosError> {
    match input {
        Handle::File(file) => {
            let mut byte = [0];
            Ok((file.read(&mut byte)? != 0).then_some(byte[0]))
        }
        Handle::Device(device) => match device.kind() {
            Device::Console(_) => Ok(keyboard.try_read_character()),
            Device::Null => Ok(Some(0)),
            Device::Auxiliary | Device::Printer => Err(DosError::AccessDenied),
        },
    }
}

pub(super) async fn read_character(input: &Handle, keyboard: &Keyboard) -> Result<u8, DosError> {
    if matches!(input, Handle::Device(device) if matches!(device.kind(), Device::Console(_))) {
        return Ok(keyboard.read_character().await);
    }
    match try_read_character(input, keyboard)? {
        Some(byte) => Ok(byte),
        // Character reads have no EOF result. File growth has no notification
        // source yet, and a redirected read must not consume console input.
        None => pending().await,
    }
}

/// Edits a line with capacity for its final carriage return.
pub(in crate::services) async fn read_line(
    input: &Keyboard,
    source: &Handle,
    capacity: u8,
    output: CharacterOutput<'_>,
) -> Result<Vec<u8>, DosError> {
    crate::console::edit_line(
        async || read_character(source, input).await,
        capacity,
        output,
    )
    .await
}
