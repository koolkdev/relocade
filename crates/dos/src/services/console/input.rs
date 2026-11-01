//! DOS character reads, readiness, and flushing over a resolved handle.

use crate::{console::CharacterOutput, error::DosError, files::Handle};
use std::future::pending;

/// DOS status queries ask the source, independently of cooked-line buffering and EOF flags.
pub(in crate::services) async fn available(input: &Handle) -> Result<bool, DosError> {
    match input {
        Handle::File(file) => Ok(file.input_available()),
        Handle::Device(handle) => handle.device.input_available().await,
    }
}

/// Discard queued input from this source without consuming file data.
pub(super) async fn flush(input: &Handle) -> Result<(), DosError> {
    if let Handle::Device(handle) = input {
        handle.device.flush_input().await?;
    }
    Ok(())
}

pub(super) async fn try_read_character(input: &Handle) -> Result<Option<u8>, DosError> {
    match input {
        Handle::File(file) => {
            let mut byte = [0];
            Ok((file.read(&mut byte)? != 0).then_some(byte[0]))
        }
        Handle::Device(handle) => handle.device.try_read_character().await,
    }
}

pub(super) async fn read_character(input: &Handle) -> Result<u8, DosError> {
    if let Handle::Device(handle) = input {
        return handle.device.read_character().await;
    }
    match try_read_character(input).await? {
        Some(byte) => Ok(byte),
        // Character reads have no EOF result. File growth has no notification
        // source yet, and a redirected read must not consume console input.
        None => pending().await,
    }
}

/// Edits a line with capacity for its final carriage return.
pub(in crate::services) async fn read_line(
    source: &Handle,
    capacity: u8,
    output: CharacterOutput<'_>,
) -> Result<Vec<u8>, DosError> {
    crate::console::edit_line(
        async || read_character(source).await,
        capacity,
        async |byte| {
            // DOS character services ignore echo failures, including redirected output.
            let _ = output.write_cooked(byte).await;
            Ok(())
        },
    )
    .await
}
