//! Byte I/O and readiness on retained file or device handles.
//!
//! These operations neither edit characters nor change DOS cooked-mode/EOF
//! flags. Interrupt services apply those policies before or after the transfer.

use super::Handle;
use crate::DosError;

impl Handle {
    /// Peeks without advancing a file cursor or consuming device input.
    pub(crate) async fn peek_character(&self) -> Result<Option<u8>, DosError> {
        match self {
            Self::File(file) => file.peek_character(),
            Self::Device(handle) => handle.device.peek_character().await,
        }
    }

    pub(crate) async fn read(&self, bytes: &mut [u8]) -> Result<usize, DosError> {
        let count = match self {
            Self::File(file) => file.read(bytes)?,
            Self::Device(handle) => handle.device.read(bytes).await?,
        };
        assert!(count <= bytes.len(), "read exceeded its buffer");
        Ok(count)
    }

    /// A zero-length file write truncates or extends the file at its cursor.
    pub(crate) async fn write(&self, bytes: &[u8]) -> Result<usize, DosError> {
        match self {
            Self::File(file) => file.write(bytes),
            Self::Device(handle) => handle.write(bytes).await,
        }
    }

    /// Queries the source, independently of cooked-line buffering and EOF flags.
    pub(crate) async fn input_available(&self) -> Result<bool, DosError> {
        match self {
            Self::File(file) => Ok(file.input_available()),
            Self::Device(handle) => handle.device.input_available().await,
        }
    }

    pub(crate) async fn output_ready(&self) -> Result<bool, DosError> {
        match self {
            Self::File(_) => Ok(true),
            Self::Device(handle) => handle.device.output_ready().await,
        }
    }

    /// Discards queued device input without consuming redirected file data.
    pub(crate) async fn flush_input(&self) -> Result<(), DosError> {
        if let Self::Device(handle) = self {
            handle.device.flush_input().await?;
        }
        Ok(())
    }
}
