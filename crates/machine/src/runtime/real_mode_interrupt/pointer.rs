//! Guest addresses are values; memory access remains with `MemoryView`.

/// A 16:16 real-mode address, captured, stored or returned by an interrupt service.
///
/// This value does not borrow memory or grant a host pointer into guest storage.
/// Access remains through `MemoryView`, including physical mappings and A20.
/// Protected-mode selectors and retained protected-mode caches are not supported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FarPointer {
    pub segment: u16,
    pub offset: u16,
}

impl FarPointer {
    pub const fn segment_base(self) -> u32 {
        (self.segment as u32) << 4
    }

    pub const fn address(self) -> u32 {
        self.segment_base() + self.offset as u32
    }

    /// Decodes the offset in the low word and the segment in the high word.
    pub const fn from_packed(value: u32) -> Self {
        Self {
            segment: (value >> 16) as u16,
            offset: value as u16,
        }
    }

    pub const fn packed(self) -> u32 {
        ((self.segment as u32) << 16) | self.offset as u32
    }
}
