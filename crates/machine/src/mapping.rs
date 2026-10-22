//! Initial mappings for real-mode physical or protected-user linear memory.

use relocade_x86::{page_table, PhysicalMemoryMap};
use std::{fmt, ops::RangeInclusive};

/// Host-managed protected-user mappings. These are not guest CR3 page tables.
pub struct PagedMemoryMap {
    bytes: Vec<u8>,
}

impl Default for PagedMemoryMap {
    fn default() -> Self {
        Self {
            bytes: vec![0; page_table::BYTE_LEN as usize],
        }
    }
}

impl PagedMemoryMap {
    /// Maps whole 4-KiB pages to contiguous backing. Aliases are allowed.
    pub fn map(
        &mut self,
        range: RangeInclusive<u32>,
        backing_offset: u32,
        writable: bool,
    ) -> Result<(), MappingError> {
        let (start, pages) = checked_pages(&range, backing_offset, 1 << 32)?;
        for index in 0..pages {
            let address = start + index * page_table::PAGE_BYTES;
            let offset = page_table::entry_offset(address) as usize;
            let entry =
                page_table::encode_entry(backing_offset + index * page_table::PAGE_BYTES, writable)
                    .expect("validated page alignment");
            self.bytes[offset..offset + 4].copy_from_slice(&entry);
        }
        Ok(())
    }

    pub fn unmap(&mut self, range: RangeInclusive<u32>) -> Result<(), MappingError> {
        let (start, pages) = checked_pages(&range, 0, 1 << 32)?;
        let offset = page_table::entry_offset(start) as usize;
        self.bytes[offset..offset + pages as usize * 4].fill(0);
        Ok(())
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn fits_backing(&self, capacity: u64) -> bool {
        self.bytes.as_chunks::<4>().0.iter().all(|entry| {
            page_table::decode_entry(*entry).is_none_or(|page| {
                u64::from(page.backing_offset) + u64::from(page_table::PAGE_BYTES) <= capacity
            })
        })
    }
}

/// Initial mapping image. Protected mappings can later be edited through the
/// live machine; the configuration remains the initial seed.
pub enum MemoryMap {
    Physical(Box<PhysicalMemoryMap>),
    Paged(PagedMemoryMap),
}

impl From<PhysicalMemoryMap> for MemoryMap {
    fn from(map: PhysicalMemoryMap) -> Self {
        Self::Physical(Box::new(map))
    }
}
impl From<PagedMemoryMap> for MemoryMap {
    fn from(map: PagedMemoryMap) -> Self {
        Self::Paged(map)
    }
}

impl MemoryMap {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Physical(map) => map.to_bytes().to_vec(),
            Self::Paged(map) => map.to_bytes().to_vec(),
        }
    }
    pub fn fits_backing(&self, capacity: u64) -> bool {
        match self {
            Self::Physical(map) => map.fits_backing(capacity),
            Self::Paged(map) => map.fits_backing(capacity),
        }
    }
}

pub(crate) fn checked_pages(
    range: &RangeInclusive<u32>,
    backing_offset: u32,
    capacity: u64,
) -> Result<(u32, u32), MappingError> {
    let start = *range.start();
    let end = *range.end();
    if start > end {
        return Err(MappingError::InvalidRange);
    }
    let bytes = u64::from(end) - u64::from(start) + 1;
    if !start.is_multiple_of(page_table::PAGE_BYTES)
        || !bytes.is_multiple_of(u64::from(page_table::PAGE_BYTES))
        || !backing_offset.is_multiple_of(page_table::PAGE_BYTES)
    {
        return Err(MappingError::UnalignedRange);
    }
    if u64::from(backing_offset) + bytes > capacity {
        return Err(MappingError::BackingRange);
    }
    Ok((start, (bytes / u64::from(page_table::PAGE_BYTES)) as u32))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingError {
    InvalidRange,
    UnalignedRange,
    BackingRange,
    PhysicalMemory,
}
impl fmt::Display for MappingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidRange => "mapping range is reversed",
            Self::UnalignedRange => "mappings require whole, aligned 4-KiB pages",
            Self::BackingRange => "mapping exceeds backing capacity",
            Self::PhysicalMemory => "live page mapping requires protected user mode",
        })
    }
}
impl std::error::Error for MappingError {}
