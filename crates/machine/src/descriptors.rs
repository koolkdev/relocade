//! Shared descriptor bytes with CPU-relative selector lookup.

mod error;
mod image;
pub use error::DescriptorError;
pub use image::DescriptorMemory;

use crate::CpuView;
use relocade_wasm_runtime::MemoryBytes;
use relocade_x86::{Descriptor, Exception, Segment, SegmentDescriptorInfo, SegmentRegister};

/// Access to dedicated descriptor storage. CPU-backed views also resolve
/// selectors through the current GDTR/LDTR, without reading unrelated CPU state.
pub struct Descriptors<'a, C = CpuView<'a>> {
    bytes: MemoryBytes<'a>,
    cpu: C,
}

/// A checked load retaining its storage and descriptor address for completion.
/// Dropping it leaves descriptor bytes unchanged. This does not lock shared storage.
pub struct ResolvedSegment<'a> {
    bytes: MemoryBytes<'a>,
    cache: SegmentRegister,
    completion: Option<u32>,
}

impl ResolvedSegment<'_> {
    pub fn cache(&self) -> SegmentRegister {
        self.cache
    }

    pub fn needs_completion(&self) -> bool {
        self.completion.is_some()
    }

    /// Sets the accessed bit after the instruction's remaining guards succeed.
    /// The returned cache can then be installed in the destination CPU register.
    pub fn complete(mut self) -> SegmentRegister {
        if let Some(address) = self.completion {
            mark_accessed_at(&mut self.bytes, address);
        }
        self.cache
    }
}

impl<'a, C> Descriptors<'a, C> {
    pub(crate) fn new(bytes: MemoryBytes<'a>, cpu: C) -> Self {
        Self { bytes, cpu }
    }

    /// Reads a bounded descriptor by byte offset, independently of any CPU.
    pub fn read_at(&self, address: u32) -> Result<Descriptor, DescriptorError> {
        check_address(address)?;
        Ok(read_at(&self.bytes, address))
    }

    /// Edits bytes without changing table registers or loaded segment caches.
    pub fn write_at(
        &mut self,
        address: u32,
        descriptor: impl Into<Descriptor>,
    ) -> Result<(), DescriptorError> {
        check_address(address)?;
        self.bytes.write(address, &descriptor.into().to_bytes());
        Ok(())
    }
}

impl<'a> Descriptors<'a> {
    pub fn read(&self, selector: u16) -> Option<Descriptor> {
        self.address(selector)
            .map(|address| read_at(&self.bytes, address))
    }

    /// Edits a slot within the active table's bounds. RPL does not select a slot.
    pub fn write(
        &mut self,
        selector: u16,
        descriptor: impl Into<Descriptor>,
    ) -> Result<(), DescriptorError> {
        let address = self
            .address(selector)
            .ok_or(DescriptorError::OutsideTable)?;
        self.write_at(address, descriptor)
    }

    pub fn query_user_segment_descriptor(&self, selector: u16) -> SegmentDescriptorInfo {
        self.read(selector)
            .map(Descriptor::query_user)
            .unwrap_or_default()
    }

    /// Resolves a load and remembers where completion sets the accessed bit.
    /// The descriptor can still be changed through other shared-memory owners.
    pub fn resolve_user_segment(
        self,
        destination: Segment,
        selector: u16,
    ) -> Result<ResolvedSegment<'a>, Exception> {
        let address = self.address(selector);
        let descriptor = address
            .map(|address| read_at(&self.bytes, address))
            .unwrap_or_default();
        let cache = descriptor.resolve_user_segment(destination, selector)?;
        Ok(ResolvedSegment {
            bytes: self.bytes,
            cache,
            completion: address.filter(|_| descriptor.needs_accessed_write()),
        })
    }

    /// Marks a code/data descriptor accessed without changing loaded caches.
    /// Separate resolution/completion callbacks use the same CPU table registers.
    pub fn mark_accessed(&mut self, selector: u16) -> Result<(), DescriptorError> {
        let address = self
            .address(selector)
            .ok_or(DescriptorError::OutsideTable)?;
        mark_accessed_at(&mut self.bytes, address);
        Ok(())
    }

    fn address(&self, selector: u16) -> Option<u32> {
        if selector & !3 == 0 {
            return None;
        }
        let (base, limit) = if selector & 4 == 0 {
            let table = self.cpu.gdtr();
            (table.base, u32::from(table.limit))
        } else {
            let table = self.cpu.ldtr();
            if table.usable == 0 {
                return None;
            }
            (table.base, table.limit)
        };
        let offset = u32::from(selector & !7);
        if offset + Descriptor::BYTE_LEN as u32 - 1 > limit {
            return None;
        }
        let address = base.checked_add(offset)?;
        check_address(address).ok()?;
        Some(address)
    }
}

fn read_at(memory: &MemoryBytes<'_>, address: u32) -> Descriptor {
    let mut bytes = [0; Descriptor::BYTE_LEN];
    memory.read(address, &mut bytes);
    Descriptor::from_bytes(bytes)
}

fn mark_accessed_at(memory: &mut MemoryBytes<'_>, address: u32) {
    let descriptor = read_at(memory, address);
    memory.write(address, &descriptor.with_accessed().to_bytes());
}

fn check_address(address: u32) -> Result<(), DescriptorError> {
    if address > DescriptorMemory::BYTE_LEN - Descriptor::BYTE_LEN as u32 {
        return Err(DescriptorError::OutsideStorage);
    }
    Ok(())
}
