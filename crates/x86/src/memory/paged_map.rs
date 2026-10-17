//! Host-managed protected-mode linear-to-backing mappings.

use super::page_table::{FRAME_MASK, PAGE_BYTES, PRESENT, WRITABLE};
use std::{fmt, ops::RangeInclusive};

/// Backing location and write permission for one resolved linear byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PagedMapping {
    pub backing_offset: u32,
    pub writable: bool,
}

/// Four-KiB page mappings for protected user-mode execution. These are the
/// engine's host-managed mappings, not guest CR3 page-directory structures.
#[derive(Debug, Default)]
pub struct PagedMemoryMap(std::collections::BTreeMap<u32, u32>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PagedMapError {
    EmptyRange,
    UnalignedRange,
    UnalignedBacking,
    BackingOverflow,
}
impl fmt::Display for PagedMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyRange => "mapping range is empty",
            Self::UnalignedRange => "mapping must contain complete 4-KiB pages",
            Self::UnalignedBacking => "mapping backing must be 4-KiB aligned",
            Self::BackingOverflow => "mapping backing exceeds the 32-bit address space",
        })
    }
}
impl std::error::Error for PagedMapError {}

impl PagedMemoryMap {
    pub const BYTE_LEN: usize = 1 << 22;

    /// Resolves a linear byte using the same entries consumed by generated code.
    pub fn get(&self, address: u32) -> Option<PagedMapping> {
        let entry = self.entry(address);
        (entry & PRESENT != 0).then_some(PagedMapping {
            backing_offset: (entry & FRAME_MASK) | (address & (PAGE_BYTES - 1)),
            writable: entry & WRITABLE != 0,
        })
    }

    /// Maps complete linear pages, replacing previous mappings in this range.
    /// Backing offsets are independent of linear addresses.
    pub fn map(
        &mut self,
        range: RangeInclusive<u32>,
        backing: u32,
        writable: bool,
    ) -> Result<(), PagedMapError> {
        let (first, last) = pages(range)?;
        if backing & 4095 != 0 {
            return Err(PagedMapError::UnalignedBacking);
        }
        let bytes = u64::from(last - first + 1) * 4096;
        if u64::from(backing) + bytes > 1 << 32 {
            return Err(PagedMapError::BackingOverflow);
        }
        for page in first..=last {
            self.0.insert(
                page,
                (backing + (page - first) * PAGE_BYTES)
                    | PRESENT
                    | if writable { WRITABLE } else { 0 },
            );
        }
        Ok(())
    }

    pub fn unmap(&mut self, range: RangeInclusive<u32>) -> Result<(), PagedMapError> {
        let (first, last) = pages(range)?;
        self.0.retain(|page, _| !(first..=last).contains(page));
        Ok(())
    }

    /// Checks that the backing for every mapped page fits the supplied size.
    pub fn fits_backing(&self, memory_size: u64) -> bool {
        self.0
            .values()
            .all(|entry| u64::from(entry & FRAME_MASK) + u64::from(PAGE_BYTES) <= memory_size)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0; Self::BYTE_LEN];
        for (&page, entry) in &self.0 {
            bytes[page as usize * 4..page as usize * 4 + 4].copy_from_slice(&entry.to_le_bytes());
        }
        bytes
    }

    fn entry(&self, address: u32) -> u32 {
        self.0.get(&(address >> 12)).copied().unwrap_or(0)
    }
}

fn pages(range: RangeInclusive<u32>) -> Result<(u32, u32), PagedMapError> {
    if range.is_empty() {
        return Err(PagedMapError::EmptyRange);
    }
    let (start, end) = range.into_inner();
    if start & 4095 != 0 || end & 4095 != 4095 {
        return Err(PagedMapError::UnalignedRange);
    }
    Ok((start >> 12, end >> 12))
}

#[cfg(test)]
mod tests;
