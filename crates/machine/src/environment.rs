use crate::MemoryAccess;
use wasm86_x86::{DescriptorTables, Exception, Segment, SegmentDescriptorInfo, StoredSegment};

/// Synchronous services supplied by the guest environment. Methods complete each transfer once;
/// asynchronous work must be handled by the embedding between execution entries.
/// Port widths are 1, 2 or 4 bytes; MMIO widths are 1 through 8, in little endian.
/// Default methods model an open bus: all-one reads and ignored writes.
/// Device state need not be `Send`; thread requirements belong to the embedding.
pub trait Environment {
    /// Resolves protected user-mode segment loads without modifying CPU or memory.
    /// Errors must be #GP, #SS or #NP. The default has no installed descriptors.
    fn resolve_segment(
        &mut self,
        segment: Segment,
        selector: u16,
    ) -> Result<StoredSegment, Exception> {
        DescriptorTables::default().resolve_user_segment(segment, selector)
    }
    fn query_segment_descriptor(&mut self, _selector: u16) -> SegmentDescriptorInfo {
        SegmentDescriptorInfo::default()
    }

    fn read_port(&mut self, _memory: &mut MemoryAccess<'_>, _port: u16, _bytes: u8) -> u32 {
        u32::MAX
    }
    fn write_port(&mut self, _memory: &mut MemoryAccess<'_>, _port: u16, _bytes: u8, _value: u32) {}
    fn read_mmio(&mut self, _memory: &mut MemoryAccess<'_>, _address: u32, _bytes: u8) -> u64 {
        u64::MAX
    }
    fn write_mmio(
        &mut self,
        _memory: &mut MemoryAccess<'_>,
        _address: u32,
        _bytes: u8,
        _value: u64,
    ) {
    }
}

impl Environment for () {}
