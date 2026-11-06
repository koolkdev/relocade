//! Output routing and cooked characters over retained console resources.

use crate::{error::DosError, files::Handle, stdio::Stdio};

impl<'a> CharacterOutput<'a> {
    pub(crate) fn handle(stdio: &'a Stdio, destination: Result<Handle, DosError>) -> Self {
        Self { stdio, destination }
    }
}

/// An output destination resolved before a service waits for input.
pub(crate) struct CharacterOutput<'a> {
    stdio: &'a Stdio,
    destination: Result<Handle, DosError>,
}

impl CharacterOutput<'_> {
    /// Expands tabs, tracks the resulting column, and returns device errors.
    pub(crate) async fn write_cooked(&self, byte: u8) -> Result<(), DosError> {
        let (byte, count) = self.stdio.cook_character(byte);
        let bytes = [byte; 8];
        self.write_all(&bytes[..usize::from(count)]).await
    }

    /// Character output preserves console EOF; only handle writes clear it.
    pub(crate) async fn write(&self, bytes: &[u8]) -> Result<usize, DosError> {
        match &self.destination {
            Err(error) => Err(*error),
            Ok(handle) => {
                if bytes.is_empty() {
                    // Character output does not use AH=40h's file truncation rule.
                    Ok(0)
                } else {
                    handle.write(bytes).await
                }
            }
        }
    }

    /// Completes cooked output even when a driver accepts only part of a write.
    pub(crate) async fn write_all(&self, mut bytes: &[u8]) -> Result<(), DosError> {
        while !bytes.is_empty() {
            let count = self.write(bytes).await?;
            if count == 0 {
                return Err(DosError::WriteFault);
            }
            bytes = &bytes[count..];
        }
        Ok(())
    }
}
