//! Host-owned routing from physical pages to backing memory or MMIO devices.

use std::{fmt, ops::RangeInclusive};

pub(super) const PAGE_SHIFT: u32 = 12;
pub(super) const PAGE_MASK: u32 = PhysicalMemoryMap::PAGE_BYTES - 1;
pub(super) const UNMAPPED: u32 = 0;
pub(super) const RAM: u32 = 1;
pub(super) const ROM: u32 = 2;
pub(super) const MMIO: u32 = 3;
pub(super) const CODE_PAGE: u32 = 1 << 2;

/// Index in an execution instance's paired MMIO operation tables.
/// Zero keeps the range resolver used for unconnected or mixed-device pages.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MmioHandlerId(u32);

impl MmioHandlerId {
    pub const GENERIC: Self = Self(0);

    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u32 {
        self.0
    }
}

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
    /// Reads and writes call this index in the live MMIO operation tables.
    Mmio { handler: MmioHandlerId },
}

impl PhysicalMapping {
    /// MMIO resolved by the bus, including sub-page boundaries and holes.
    pub const MMIO: Self = Self::Mmio {
        handler: MmioHandlerId::GENERIC,
    };

    fn backing_offset(self) -> Option<u32> {
        match self {
            Self::Unmapped | Self::Mmio { .. } => None,
            Self::Ram { backing_offset } | Self::Rom { backing_offset } => Some(backing_offset),
        }
    }

    fn from_entry(entry: &[u8; 8]) -> Self {
        let payload = u32::from_le_bytes(entry[4..].try_into().unwrap());
        match u32::from(entry[0]) & !CODE_PAGE {
            UNMAPPED => Self::Unmapped,
            RAM => Self::Ram {
                backing_offset: payload,
            },
            ROM => Self::Rom {
                backing_offset: payload,
            },
            MMIO => Self::Mmio {
                handler: MmioHandlerId::new(payload),
            },
            _ => unreachable!("only physical mapping kinds are encoded"),
        }
    }

    fn entry(self) -> [u8; 8] {
        let (kind, payload) = match self {
            Self::Unmapped => (UNMAPPED, 0),
            Self::Ram { backing_offset } => (RAM, backing_offset),
            Self::Rom { backing_offset } => (ROM, backing_offset),
            Self::Mmio { handler } => (MMIO, handler.index()),
        };
        let mut entry = [0; 8];
        entry[..4].copy_from_slice(&kind.to_le_bytes());
        entry[4..].copy_from_slice(&payload.to_le_bytes());
        entry
    }

    fn advance(self, bytes: u32) -> Self {
        match self {
            Self::Unmapped | Self::Mmio { .. } => self,
            Self::Ram { backing_offset } => Self::Ram {
                backing_offset: backing_offset + bytes,
            },
            Self::Rom { backing_offset } => Self::Rom {
                backing_offset: backing_offset + bytes,
            },
        }
    }
}

/// Host routing for the 32-bit physical address space. Each 4-KiB page has
/// one mapping. Unspecified pages are unmapped, and several physical ranges
/// may share backing. The host allocates RAM and ROM backing separately.
///
/// This type constructs routing metadata. The host owns backing allocation,
/// device behavior and updates to the installed Wasm table.
///
/// ```
/// use relocade_x86::{PhysicalMapError, PhysicalMapping, PhysicalMemoryMap};
///
/// let map = PhysicalMemoryMap::new([
///     (0..=0x9ffff, PhysicalMapping::Ram { backing_offset: 0 }),
///     (0xf0000..=0xfffff, PhysicalMapping::Rom { backing_offset: 0xa0000 }),
/// ])?;
/// assert_eq!(map.get(0xf1234), PhysicalMapping::Rom { backing_offset: 0xa1234 });
/// assert_eq!(map.get(0xa0000), PhysicalMapping::Unmapped);
/// # Ok::<(), PhysicalMapError>(())
/// ```
#[derive(Clone, Debug)]
pub struct PhysicalMemoryMap {
    image: Box<[u8]>,
}

impl PhysicalMemoryMap {
    pub const PAGE_BYTES: u32 = 1 << PAGE_SHIFT;
    pub const PAGE_COUNT: usize = 1 << 20;
    /// Size of one serialized routing entry, in bytes.
    pub const ENTRY_BYTES: usize = 8;
    /// The trailing address-mask word. Page-entry offsets are independent of A20.
    pub const A20_MASK_OFFSET: usize = Self::PAGE_COUNT * Self::ENTRY_BYTES;
    pub const BYTE_LEN: usize = Self::A20_MASK_OFFSET + 4;

    fn entries(&self) -> &[[u8; Self::ENTRY_BYTES]] {
        self.image.as_chunks().0
    }

    /// Checks all RAM/ROM backing, including pages currently hidden by A20.
    pub fn fits_backing(&self, memory_size: u64) -> bool {
        self.entries()
            .iter()
            .filter_map(|entry| PhysicalMapping::from_entry(entry).backing_offset())
            .all(|offset| u64::from(offset) + u64::from(Self::PAGE_BYTES) <= memory_size)
    }

    /// Physical page addresses and the byte offsets of their serialized MMIO
    /// handler words. Includes pages hidden by A20. Device publication updates
    /// the underlying routing independently of the current address gate.
    pub fn mmio_handler_offsets(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.entries()
            .iter()
            .enumerate()
            .filter_map(|(page, entry)| {
                (u32::from(entry[0]) == MMIO).then_some((
                    page as u32 * Self::PAGE_BYTES,
                    (page * Self::ENTRY_BYTES + 4) as u32,
                ))
            })
    }

    /// Constructs a table from inclusive physical ranges. Empty input leaves all
    /// pages unmapped; later regions replace earlier overlapping regions.
    /// A20 starts disabled for DOS-compatible address wrapping.
    /// Each range must contain complete pages.
    /// For RAM/ROM, `backing_offset` names the first byte and must be page aligned;
    /// subsequent pages use consecutive backing. Backing must fit the 32-bit
    /// address space, but its allocation remains the host's duty.
    pub fn new(
        regions: impl IntoIterator<Item = (RangeInclusive<u32>, PhysicalMapping)>,
    ) -> Result<Self, PhysicalMapError> {
        let mut map = Self {
            image: vec![0; Self::BYTE_LEN].into_boxed_slice(),
        };
        map.set_a20_enabled(false);
        let (pages, _) = map.image.as_chunks_mut::<8>();
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
                *page = mapping.advance((index as u32) << PAGE_SHIFT).entry();
            }
        }
        Ok(map)
    }

    /// Marks the backing page containing `backing_offset` as containing tracked code.
    /// All RAM/ROM aliases receive the flag, including pages hidden by A20.
    /// Reads and ROM write policy are unchanged; RAM writes use the host callback.
    /// The host coordinates tracking and installs the updated image while execution
    /// is stopped. This metadata alone does not invalidate compiled code.
    pub fn set_code_page(&mut self, backing_offset: u32, contains_code: bool) {
        let page = backing_offset & !PAGE_MASK;
        let (entries, _) = self.image.as_chunks_mut::<8>();
        for entry in entries {
            if PhysicalMapping::from_entry(entry).backing_offset() == Some(page) {
                if contains_code {
                    entry[0] |= CODE_PAGE as u8;
                } else {
                    entry[0] &= !(CODE_PAGE as u8);
                }
            }
        }
    }

    /// Selects the platform A20 gate. New maps disable it. The host installs the
    /// updated serialized mask before the next transfer that should observe it.
    pub fn set_a20_enabled(&mut self, enabled: bool) {
        let mask = if enabled { u32::MAX } else { !(1 << 20) };
        self.image[Self::A20_MASK_OFFSET..].copy_from_slice(&mask.to_le_bytes());
    }

    /// Applies the platform A20 gate to an address on the physical bus.
    /// MMIO devices receive this address, independently of RAM backing offsets.
    pub fn bus_address(&self, address: u32) -> u32 {
        let mask = u32::from_le_bytes(self.image[Self::A20_MASK_OFFSET..].try_into().unwrap());
        address & mask
    }

    /// Applies A20 and resolves one byte, adjusting direct offsets to that byte.
    /// This lookup performs no memory transfer or fault check.
    pub fn get(&self, address: u32) -> PhysicalMapping {
        let address = self.bus_address(address);
        let entry = &self.entries()[(address >> PAGE_SHIFT) as usize];
        PhysicalMapping::from_entry(entry).advance(address & PAGE_MASK)
    }

    /// Borrows the routing image installed at offset zero of `physicalMap`. Each
    /// eight-byte entry holds a little-endian u32 kind (0 = unmapped, 1 = RAM,
    /// 2 = ROM, 3 = MMIO) and a little-endian u32 backing-page offset or MMIO
    /// handler index. Kind bit 2 marks tracked RAM/ROM. Unmapped payloads are
    /// zero. MMIO index zero selects generic range resolution.
    ///
    /// The trailing u32 mask is FFFFFFFF with A20 enabled and FFEFFFFF otherwise.
    pub fn as_bytes(&self) -> &[u8] {
        &self.image
    }

    /// Copies the routing image. Use [`Self::as_bytes`] to borrow it for installation.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.image.to_vec()
    }
}

/// Invalid host mapping configuration, independent of guest execution faults.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalMapError {
    EmptyRange,
    UnalignedRange,
    UnalignedBacking,
    BackingOverflow,
}

impl fmt::Display for PhysicalMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyRange => "physical mapping range is empty",
            Self::UnalignedRange => "physical mapping range must contain complete 4-KiB pages",
            Self::UnalignedBacking => "physical mapping backing must be 4-KiB aligned",
            Self::BackingOverflow => "physical mapping backing exceeds the 32-bit address space",
        })
    }
}

impl std::error::Error for PhysicalMapError {}

#[cfg(test)]
mod tests;
