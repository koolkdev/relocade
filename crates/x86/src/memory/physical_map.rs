//! Host-owned routing from physical pages to backing memory or MMIO devices.

use std::{fmt, ops::RangeInclusive};

pub(super) const PAGE_SHIFT: u32 = 12;
pub(super) const PAGE_MASK: u32 = PhysicalMemoryMap::PAGE_BYTES - 1;
pub(super) const UNMAPPED: u32 = 0;
pub(super) const RAM: u32 = 1;
pub(super) const ROM: u32 = 2;
pub(super) const MMIO: u32 = 3;

/// Where a physical address is routed. Backing offsets address bytes in the
/// host's Wasm backing memory; they are not guest physical addresses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PhysicalMapping {
    /// Reads return FF bytes and writes are ignored. This is the physical
    /// backend's hole policy, not an architectural access fault.
    #[default]
    Unmapped,
    /// Reads and writes access backing memory directly.
    Ram { backing_offset: u32 },
    /// Reads access backing memory directly; writes are ignored.
    Rom { backing_offset: u32 },
    /// Reads and writes use the host's MMIO callbacks.
    Mmio,
}

impl PhysicalMapping {
    fn backing_offset(self) -> Option<u32> {
        match self {
            Self::Unmapped | Self::Mmio => None,
            Self::Ram { backing_offset } | Self::Rom { backing_offset } => Some(backing_offset),
        }
    }

    fn advance(self, bytes: u32) -> Self {
        match self {
            Self::Unmapped | Self::Mmio => self,
            Self::Ram { backing_offset } => Self::Ram {
                backing_offset: backing_offset + bytes,
            },
            Self::Rom { backing_offset } => Self::Rom {
                backing_offset: backing_offset + bytes,
            },
        }
    }
}

/// Host routing for ordinary real mode, in complete 4-KiB pages. The fixed table
/// covers physical addresses below 0x110000: enough for every `selector << 4`
/// base plus a 16-bit offset, with A20 enabled. Unspecified pages are unmapped.
/// Each page has one mapping; mixed RAM/ROM/MMIO regions within a page are not
/// represented. Several physical ranges may share backing.
///
/// This type constructs routing metadata. The host owns backing allocation,
/// device behavior and updates to the installed Wasm table.
///
/// ```
/// use wasm86_x86::{PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};
///
/// let map = PhysicalMemoryMap::new([
///     (0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 }),
///     (0xf0000..=0xfffff, PhysicalMapping::Rom { backing_offset: 0xa0000 }),
/// ])?;
/// assert_eq!(map.get(0xf1234), PhysicalMapping::Rom { backing_offset: 0xa1234 });
/// assert_eq!(map.get(0xa0000), PhysicalMapping::Unmapped);
/// # Ok::<(), PhysicalMapError>(())
/// ```
#[derive(Debug)]
pub struct PhysicalMemoryMap {
    pages: [PhysicalMapping; Self::PAGE_COUNT],
}

impl PhysicalMemoryMap {
    pub const PAGE_BYTES: u32 = 1 << PAGE_SHIFT;
    pub const PAGE_COUNT: usize = 272;
    pub const BYTE_LEN: usize = Self::PAGE_COUNT * 8;

    /// Constructs a table from inclusive physical ranges. Empty input leaves all
    /// pages unmapped; later regions replace earlier overlapping regions.
    /// Each range must contain complete pages within 0..=0x10ffff.
    /// For RAM/ROM, `backing_offset` names the first byte and must be page aligned;
    /// subsequent pages use consecutive backing. Backing must fit the 32-bit
    /// address space, but its allocation remains the host's duty.
    pub fn new(
        regions: impl IntoIterator<Item = (RangeInclusive<u32>, PhysicalMapping)>,
    ) -> Result<Self, PhysicalMapError> {
        let mut pages = [PhysicalMapping::Unmapped; Self::PAGE_COUNT];
        for (range, mapping) in regions {
            if range.is_empty() {
                return Err(PhysicalMapError::EmptyRange);
            }
            let (start, end) = range.into_inner();
            if start & PAGE_MASK != 0 || end & PAGE_MASK != PAGE_MASK {
                return Err(PhysicalMapError::UnalignedRange);
            }
            let first_page = (start >> PAGE_SHIFT) as usize;
            let end_page = (end >> PAGE_SHIFT) as usize + 1;
            if end_page > Self::PAGE_COUNT {
                return Err(PhysicalMapError::RangeOutOfBounds);
            }
            if let Some(backing) = mapping.backing_offset() {
                if backing & PAGE_MASK != 0 {
                    return Err(PhysicalMapError::UnalignedBacking);
                }
                let bytes = u64::from(end) - u64::from(start) + 1;
                if u64::from(backing) + bytes > 1 << 32 {
                    return Err(PhysicalMapError::BackingOverflow);
                }
            }
            for (index, page) in pages[first_page..end_page].iter_mut().enumerate() {
                *page = mapping.advance((index as u32) << PAGE_SHIFT);
            }
        }
        Ok(Self { pages })
    }

    /// Resolves one physical byte for inspection, adjusting direct offsets to
    /// that byte. Addresses outside the table return [`PhysicalMapping::Unmapped`].
    /// This lookup performs no memory transfer or fault check.
    pub fn get(&self, address: u32) -> PhysicalMapping {
        self.pages
            .get((address >> PAGE_SHIFT) as usize)
            .copied()
            .unwrap_or_default()
            .advance(address & PAGE_MASK)
    }

    /// Serializes 272 entries of eight bytes each, without a header. Each entry
    /// contains a little-endian u32 kind (0 = unmapped, 1 = RAM, 2 = ROM, 3 = MMIO)
    /// followed by a u32 backing-page offset. Unmapped and MMIO offsets are zero.
    /// Physical page `p` starts at byte `p * 8` within the 2176-byte image.
    /// This format uses explicit fields, independent of Rust's enum layout.
    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        for (entry, page) in bytes.chunks_exact_mut(8).zip(&self.pages) {
            let (kind, backing): (u32, u32) = match *page {
                PhysicalMapping::Unmapped => (UNMAPPED, 0),
                PhysicalMapping::Ram { backing_offset } => (RAM, backing_offset),
                PhysicalMapping::Rom { backing_offset } => (ROM, backing_offset),
                PhysicalMapping::Mmio => (MMIO, 0),
            };
            entry[..4].copy_from_slice(&kind.to_le_bytes());
            entry[4..].copy_from_slice(&backing.to_le_bytes());
        }
        bytes
    }
}

/// Invalid host mapping configuration, independent of guest execution faults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalMapError {
    EmptyRange,
    UnalignedRange,
    RangeOutOfBounds,
    UnalignedBacking,
    BackingOverflow,
}

impl fmt::Display for PhysicalMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyRange => "physical mapping range is empty",
            Self::UnalignedRange => "physical mapping range must contain complete 4-KiB pages",
            Self::RangeOutOfBounds => "physical mapping range exceeds the real-mode table",
            Self::UnalignedBacking => "physical mapping backing must be 4-KiB aligned",
            Self::BackingOverflow => "physical mapping backing exceeds the 32-bit address space",
        })
    }
}

impl std::error::Error for PhysicalMapError {}

#[cfg(test)]
mod tests;
