//! The live linear page table shared by flat CPUs and host memory views.

use super::{range_fits, MemoryError};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use relocade_x86::{abi, page_table};
use std::{
    ops::{Range, RangeInclusive},
    sync::{Arc, RwLock, RwLockReadGuard},
};

/// A host-managed 32-bit linear address space with 4-KiB pages.
#[derive(Clone, crate::worker::Transfer)]
pub struct FlatPageTable {
    pub(crate) entries: SharedMemory,
    backing_size: u64,
    access: Arc<RwLock<()>>,
}

impl FlatPageTable {
    pub(super) fn new(backing_size: u64) -> crate::Result<Self> {
        let pages = abi::PAGE_TABLE.minimum;
        let entries = SharedMemory::new(pages, pages)?;
        let table = Self {
            entries,
            backing_size,
            access: Arc::default(),
        };
        if backing_size != 0 {
            table.map_pages(0..=(backing_size - 1) as u32, 0, true)?;
        }
        Ok(table)
    }

    /// Maps whole linear pages onto consecutive backing pages. Multiple linear
    /// addresses may alias one backing page with different write permissions.
    /// Returns `MappingInUse` if a CPU or host transfer is using this table.
    pub fn map_pages(
        &self,
        range: RangeInclusive<u32>,
        backing_offset: u32,
        writable: bool,
    ) -> Result<(), MemoryError> {
        let (start, bytes) = page_range(range)?;
        if !backing_offset.is_multiple_of(page_table::PAGE_BYTES) {
            return Err(MemoryError::UnalignedMapping);
        }
        if u64::from(backing_offset) + bytes > self.backing_size {
            return Err(MemoryError::BackingRange);
        }
        let entries: Vec<u8> = (0..bytes)
            .step_by(page_table::PAGE_BYTES as usize)
            .flat_map(|offset| {
                page_table::encode_entry(backing_offset + offset as u32, writable)
                    .expect("aligned backing page")
            })
            .collect();
        self.replace_entries(start, &entries)
    }

    /// Removes whole linear pages without changing their backing bytes.
    pub fn unmap_pages(&self, range: RangeInclusive<u32>) -> Result<(), MemoryError> {
        let (start, bytes) = page_range(range)?;
        let pages = (bytes / u64::from(page_table::PAGE_BYTES)) as usize;
        self.replace_entries(start, &page_table::UNMAPPED_ENTRY.repeat(pages))
    }

    fn replace_entries(&self, start: u32, entries: &[u8]) -> Result<(), MemoryError> {
        let _guard = self
            .access
            .try_write()
            .map_err(|_| MemoryError::MappingInUse)?;
        self.entries.write(page_table::entry_offset(start), entries);
        Ok(())
    }

    // Wasm may cache a translation for the duration of an execution call.
    pub(crate) fn enter(&self) -> RwLockReadGuard<'_, ()> {
        self.access.read().unwrap()
    }

    /// Check the whole range before permitting any partial transfer.
    pub(super) fn transfer(
        &self,
        address: u32,
        len: usize,
        write: bool,
        mut transfer: impl FnMut(u32, Range<usize>),
    ) -> Result<(), MemoryError> {
        if !range_fits(address, len, 1 << 32) {
            return Err(MemoryError::AddressRange);
        }
        let _guard = self.enter();
        let mut spans = Vec::new();
        let mut start = 0;
        while start < len {
            let linear = address + start as u32;
            let mut entry = [0; 4];
            self.entries
                .read(page_table::entry_offset(linear), &mut entry);
            let backing = page_table::resolve_entry(entry, linear, write)
                .map_err(MemoryError::Translation)?;
            let remaining = page_table::PAGE_BYTES - linear % page_table::PAGE_BYTES;
            let end = start + (remaining as usize).min(len - start);
            if start == 0 && end == len {
                transfer(backing, 0..len);
                return Ok(());
            }
            spans.push((backing, start..end));
            start = end;
        }
        for (backing, range) in spans {
            transfer(backing, range);
        }
        Ok(())
    }
}

fn page_range(range: RangeInclusive<u32>) -> Result<(u32, u64), MemoryError> {
    let (start, end) = range.into_inner();
    if start > end {
        return Err(MemoryError::AddressRange);
    }
    let bytes = u64::from(end) - u64::from(start) + 1;
    if !start.is_multiple_of(page_table::PAGE_BYTES)
        || !bytes.is_multiple_of(u64::from(page_table::PAGE_BYTES))
    {
        return Err(MemoryError::UnalignedMapping);
    }
    Ok((start, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MachineConfig, Memory};

    #[test]
    fn complete_ranges_are_validated_before_any_page_or_byte_changes() {
        let memory = Memory::new(MachineConfig::flat(0x2000).unwrap()).unwrap();
        let pages = memory.page_table().unwrap();
        let mut view = memory.view();
        view.write_u16(0xfff, 0x1234).unwrap();
        assert_eq!(
            pages.map_pages(0..=0x1fff, 0x1000, true),
            Err(MemoryError::BackingRange)
        );
        assert_eq!(
            pages.unmap_pages(0..=0xffe),
            Err(MemoryError::UnalignedMapping)
        );
        pages.map_pages(0x1000..=0x1fff, 0x1000, false).unwrap();
        assert!(view.write_u16(0xfff, 0xffff).is_err());
        assert_eq!(view.read_u16(0xfff).unwrap(), 0x1234);

        // A running CPU holds this guard until it can discard cached translations.
        let running = pages.enter();
        assert_eq!(
            pages.unmap_pages(0..=0x1fff),
            Err(MemoryError::MappingInUse)
        );
        drop(running);
        pages.unmap_pages(0x1000..=0x1fff).unwrap();
        let mut output = [9; 2];
        assert!(view.read(0xfff, &mut output).is_err());
        assert_eq!(output, [9; 2]);
    }
}
