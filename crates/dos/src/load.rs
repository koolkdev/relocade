//! Host-supplied inputs for DOS process initialization.

use crate::{process, LoadError};

/// Inputs shared by COM and MZ loading. Byte strings use the guest's encoding.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoadOptions<'a> {
    /// Raw command tail, including any leading space, without its length or CR.
    /// At most 126 bytes fit alongside the length and terminator in the PSP.
    /// The loader copies these bytes without parsing or quoting arguments.
    pub command_tail: &'a [u8],
}

impl LoadOptions<'_> {
    pub(crate) fn validate(&self) -> Result<(), LoadError> {
        let capacity = process::PSP_BYTES - usize::from(process::COMMAND_TAIL_OFFSET) - 2;
        if self.command_tail.len() > capacity {
            return Err(LoadError::CommandTailTooLong);
        }
        Ok(())
    }
}
