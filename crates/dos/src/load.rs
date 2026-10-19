//! Host-supplied inputs for DOS process initialization.

use crate::{process, LoadError};

/// Inputs shared by COM and MZ loading. Byte strings use the guest's encoding.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoadOptions<'a> {
    /// Raw command tail, including any leading space, without its length or CR.
    /// At most 126 bytes fit alongside the length and terminator in the PSP.
    /// The loader copies these bytes without parsing or quoting arguments.
    pub command_tail: &'a [u8],
    /// Environment entries in `NAME=VALUE` form, without NUL terminators.
    /// Entries are copied in order, without changing their case or encoding.
    /// The complete block, including the executable path, must fit in 64 KiB.
    pub environment: &'a [&'a [u8]],
    /// Executable path stored after the environment, without a NUL terminator.
    /// Supply the full DOS path when known; the default is an empty string.
    pub program_path: &'a [u8],
}

impl LoadOptions<'_> {
    pub(crate) fn validate_command_tail(&self) -> Result<(), LoadError> {
        let capacity = process::PSP_BYTES - usize::from(process::COMMAND_TAIL_OFFSET) - 2;
        if self.command_tail.len() > capacity {
            return Err(LoadError::CommandTailTooLong);
        }
        Ok(())
    }

    pub(crate) fn environment_block(&self) -> Result<Vec<u8>, LoadError> {
        // DOS scans this block with 16-bit offsets. Include every terminator
        // and the DOS 3+ word counting the executable-path string.
        const SEGMENT_BYTES: usize = 0x10000;
        if self.program_path.contains(&0) {
            return Err(LoadError::InvalidEnvironment);
        }
        let mut bytes = Vec::new();
        for entry in self.environment {
            if entry.contains(&0)
                || !entry
                    .iter()
                    .position(|&byte| byte == b'=')
                    .is_some_and(|index| index > 0)
            {
                return Err(LoadError::InvalidEnvironment);
            }
            if entry.len() >= SEGMENT_BYTES - bytes.len() {
                return Err(LoadError::EnvironmentTooLarge);
            }
            bytes.extend_from_slice(entry);
            bytes.push(0);
        }
        // Even an empty environment begins with two zero bytes.
        if bytes.is_empty() {
            bytes.push(0);
        }
        let trailer_bytes = 1 + 2 + 1; // Extra NUL, string count and path terminator.
        if bytes.len() + trailer_bytes > SEGMENT_BYTES
            || self.program_path.len() > SEGMENT_BYTES - bytes.len() - trailer_bytes
        {
            return Err(LoadError::EnvironmentTooLarge);
        }
        bytes.extend_from_slice(&[0, 1, 0]);
        bytes.extend_from_slice(self.program_path);
        bytes.push(0);
        Ok(bytes)
    }
}
