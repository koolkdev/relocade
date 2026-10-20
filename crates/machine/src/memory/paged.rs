use super::{page_span, Memory};
use crate::{mapping::checked_pages, MachineStorage, MappingError, MemoryMap};
use relocade_x86::{page_table, Exception};
use std::ops::RangeInclusive;

impl<S: MachineStorage + ?Sized> Memory<'_, S> {
    /// Replaces protected-user mappings while execution is stopped. Backing
    /// bounds are checked before any change. Callers must invalidate code and
    /// dispatch entries whose assumptions depend on changed mappings.
    pub fn map_pages(
        &mut self,
        range: RangeInclusive<u32>,
        backing_offset: u32,
        writable: bool,
    ) -> Result<(), MappingError> {
        if !matches!(self.config.memory_map(), MemoryMap::Paged(_)) {
            return Err(MappingError::PhysicalMemory);
        }
        let (start, pages) = checked_pages(&range, backing_offset, self.config.memory_size())?;
        for index in 0..pages {
            let address = start + index * page_table::PAGE_BYTES;
            let entry =
                page_table::encode_entry(backing_offset + index * page_table::PAGE_BYTES, writable)
                    .expect("validated page alignment");
            self.storage
                .write_mapping(page_table::entry_offset(address), &entry);
        }
        Ok(())
    }

    pub fn unmap_pages(&mut self, range: RangeInclusive<u32>) -> Result<(), MappingError> {
        if !matches!(self.config.memory_map(), MemoryMap::Paged(_)) {
            return Err(MappingError::PhysicalMemory);
        }
        let (start, pages) = checked_pages(&range, 0, 1 << 32)?;
        for index in 0..pages {
            self.storage.write_mapping(
                page_table::entry_offset(start + index * page_table::PAGE_BYTES),
                &page_table::UNMAPPED_ENTRY,
            );
        }
        Ok(())
    }

    pub(super) fn read_paged(
        &self,
        mut address: u32,
        mut bytes: &mut [u8],
    ) -> Result<(), Exception> {
        self.check_pages(address, bytes.len(), false)?;
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            let (part, rest) = bytes.split_at_mut(count);
            let backing = self.page_address(address, false)?;
            self.storage.read_backing(backing, part);
            address = address.wrapping_add(count as u32);
            bytes = rest;
        }
        Ok(())
    }

    pub(super) fn write_paged(
        &mut self,
        mut address: u32,
        mut bytes: &[u8],
    ) -> Result<(), Exception> {
        self.check_pages(address, bytes.len(), true)?;
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            let backing = self.page_address(address, true)?;
            self.storage.write_backing(backing, &bytes[..count]);
            address = address.wrapping_add(count as u32);
            bytes = &bytes[count..];
        }
        Ok(())
    }

    fn check_pages(
        &self,
        mut address: u32,
        mut bytes: usize,
        write: bool,
    ) -> Result<(), Exception> {
        while bytes != 0 {
            self.page_address(address, write)?;
            let count = page_span(address, bytes);
            address = address.wrapping_add(count as u32);
            bytes -= count;
        }
        Ok(())
    }

    fn page_address(&self, address: u32, write: bool) -> Result<u32, Exception> {
        let mut entry = [0; 4];
        self.storage
            .read_mapping(page_table::entry_offset(address), &mut entry);
        page_table::resolve_entry(entry, address, write)
    }
}
