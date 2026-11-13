//! Execution identities keep segmented block keys separate from address hooks.

use crate::{ConfigurationError, FarPointer, Result};
use relocade_x86::ExecutionProfile;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum EntryKey {
    Address(u32),
    RealMode(u32),
}

impl EntryKey {
    pub(crate) fn block(profile: ExecutionProfile, address: u32, eip: u32) -> Result<Self> {
        if profile != ExecutionProfile::Real16 {
            return Ok(Self::Address(address));
        }
        let offset = u16::try_from(eip).map_err(|_| ConfigurationError::IncompatibleCpu)?;
        let base = address
            .checked_sub(eip)
            .filter(|base| base & 15 == 0 && *base <= 0xffff0)
            .ok_or(ConfigurationError::IncompatibleCpu)?;
        Ok(Self::RealMode(
            FarPointer {
                segment: (base >> 4) as u16,
                offset,
            }
            .packed(),
        ))
    }

    pub(crate) fn address(self) -> u32 {
        match self {
            Self::Address(address) => address,
            Self::RealMode(key) => FarPointer::from_packed(key).address(),
        }
    }
}

pub(crate) struct ModuleEntry<'a> {
    pub(crate) key: EntryKey,
    pub(crate) export: &'a str,
}

impl<'a> From<&super::DispatchEntry<'a>> for ModuleEntry<'a> {
    fn from(entry: &super::DispatchEntry<'a>) -> Self {
        Self {
            key: EntryKey::Address(entry.address),
            export: entry.export,
        }
    }
}
