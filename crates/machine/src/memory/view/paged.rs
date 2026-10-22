use super::{page_span, MemoryView};
use crate::{mapping::checked_pages, MappingError, MemoryMap};
use relocade_wasm_runtime::MemoryBytes;
use relocade_x86::{page_table, Exception};
use std::ops::RangeInclusive;

impl MemoryView<'_> {
    /// Replaces whole pages. The embedding must coordinate this change with
    /// guest execution and other users of the shared address space.
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
            self.mapping
                .as_mut()
                .expect("page table")
                .write(page_table::entry_offset(address), &entry);
        }
        super::super::version::changed(&mut self.revision);
        Ok(())
    }

    pub fn unmap_pages(&mut self, range: RangeInclusive<u32>) -> Result<(), MappingError> {
        if !matches!(self.config.memory_map(), MemoryMap::Paged(_)) {
            return Err(MappingError::PhysicalMemory);
        }
        let (start, pages) = checked_pages(&range, 0, 1 << 32)?;
        for index in 0..pages {
            self.mapping.as_mut().expect("page table").write(
                page_table::entry_offset(start + index * page_table::PAGE_BYTES),
                &page_table::UNMAPPED_ENTRY,
            );
        }
        super::super::version::changed(&mut self.revision);
        Ok(())
    }

    pub(super) fn write_paged(
        &mut self,
        mut address: u32,
        mut bytes: &[u8],
    ) -> Result<(), Exception> {
        check_pages(
            self.mapping.as_ref().expect("page table"),
            address,
            bytes.len(),
            true,
        )?;
        while !bytes.is_empty() {
            let count = page_span(address, bytes.len());
            let backing = page_address(self.mapping.as_ref().expect("page table"), address, true)
                .expect("page mappings must remain unchanged during a transfer");
            self.backing.write(backing, &bytes[..count]);
            address = address.wrapping_add(count as u32);
            bytes = &bytes[count..];
        }
        Ok(())
    }
}

pub(super) fn check_pages(
    mapping: &MemoryBytes<'_>,
    mut address: u32,
    mut bytes: usize,
    write: bool,
) -> Result<(), Exception> {
    while bytes != 0 {
        page_address(mapping, address, write)?;
        let count = page_span(address, bytes);
        address = address.wrapping_add(count as u32);
        bytes -= count;
    }
    Ok(())
}

pub(super) fn page_address(
    mapping: &MemoryBytes<'_>,
    address: u32,
    write: bool,
) -> Result<u32, Exception> {
    let mut entry = [0; 4];
    mapping.read(page_table::entry_offset(address), &mut entry);
    page_table::resolve_entry(entry, address, write)
}
