//! Host-managed protected-mode linear-to-backing mappings.

use std::{fmt, ops::RangeInclusive};

/// Four-KiB page mappings for protected user-mode execution. These are the
/// engine's host-managed mappings, not guest CR3 page-directory structures.
#[derive(Debug, Default)]
pub struct PagedMemoryMap(std::collections::BTreeMap<u32, u32>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingError {
    EmptyRange,
    UnalignedRange,
    UnalignedBacking,
    BackingOverflow,
}
impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyRange => "mapping range is empty",
            Self::UnalignedRange => "mapping must contain complete 4-KiB pages",
            Self::UnalignedBacking => "mapping backing must be 4-KiB aligned",
            Self::BackingOverflow => "mapping backing exceeds the 32-bit address space",
        })
    }
}
impl std::error::Error for MappingError {}

impl PagedMemoryMap {
    pub const BYTE_LEN: usize = 1 << 22;

    /// Maps complete linear pages, replacing previous mappings in this range.
    /// Backing offsets are independent of linear addresses.
    pub fn map(
        &mut self,
        range: RangeInclusive<u32>,
        backing: u32,
        writable: bool,
    ) -> Result<(), MappingError> {
        let (first, last) = pages(range)?;
        if backing & 4095 != 0 {
            return Err(MappingError::UnalignedBacking);
        }
        let bytes = u64::from(last - first + 1) * 4096;
        if u64::from(backing) + bytes > 1 << 32 {
            return Err(MappingError::BackingOverflow);
        }
        for page in first..=last {
            self.0.insert(
                page,
                (backing + (page - first) * 4096) | 1 | (u32::from(writable) << 1),
            );
        }
        Ok(())
    }

    pub fn unmap(&mut self, range: RangeInclusive<u32>) -> Result<(), MappingError> {
        let (first, last) = pages(range)?;
        self.0.retain(|page, _| !(first..=last).contains(page));
        Ok(())
    }

    pub fn required_backing_bytes(&self) -> u64 {
        self.0
            .values()
            .map(|entry| u64::from(entry & !4095) + 4096)
            .max()
            .unwrap_or(0)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0; Self::BYTE_LEN];
        for (&page, entry) in &self.0 {
            bytes[page as usize * 4..page as usize * 4 + 4].copy_from_slice(&entry.to_le_bytes());
        }
        bytes
    }

    pub(super) fn entry(&self, address: u32) -> u32 {
        self.0.get(&(address >> 12)).copied().unwrap_or(0)
    }
}

fn pages(range: RangeInclusive<u32>) -> Result<(u32, u32), MappingError> {
    if range.is_empty() {
        return Err(MappingError::EmptyRange);
    }
    let (start, end) = range.into_inner();
    if start & 4095 != 0 || end & 4095 != 4095 {
        return Err(MappingError::UnalignedRange);
    }
    Ok((start >> 12, end >> 12))
}

#[cfg(test)]
mod tests;
