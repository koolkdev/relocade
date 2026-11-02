//! DOS character and edited-line reads over retained input/output destinations.

use crate::{console::CharacterOutput, error::DosError, files::Handle};
use std::future::pending;

pub(super) async fn try_read_character(input: &Handle) -> Result<Option<u8>, DosError> {
    match input {
        Handle::File(_) => {
            let mut byte = [0];
            Ok((input.read(&mut byte).await? != 0).then_some(byte[0]))
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
pub(super) async fn read_line(
    source: &Handle,
    capacity: u8,
    output: &CharacterOutput<'_>,
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
