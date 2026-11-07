//! Architectural real-mode addresses, interrupt vectors and stack frames.

/// A 16:16 real-mode address for guest memory or interrupt arguments and results.
///
/// This value does not borrow memory or grant a host pointer into guest storage.
/// The embedding owns memory access, including physical mappings and A20.
/// Protected-mode selectors and retained protected-mode caches are not supported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FarPointer {
    pub segment: u16,
    pub offset: u16,
}

impl FarPointer {
    /// An offset word followed by a segment word in guest memory.
    pub const BYTE_LEN: usize = 4;
    pub const SEGMENT_OFFSET: u32 = 2;

    /// Advances the 16-bit offset, wrapping without changing the segment.
    pub const fn wrapping_add(self, bytes: usize) -> Self {
        Self {
            offset: self.offset.wrapping_add(bytes as u16),
            ..self
        }
    }

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

/// Address of a vector in the conventional IVT at 0000:0000.
pub const fn vector_address(vector: u8) -> FarPointer {
    FarPointer {
        segment: 0,
        offset: vector as u16 * FarPointer::BYTE_LEN as u16,
    }
}

/// The three words pushed by real-mode interrupt and exception entry.
/// Real-mode exceptions do not push an error code (Intel SDM Vol. 3B, 21.1.4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptFrame {
    pub return_address: FarPointer,
    pub flags: u16,
}

impl InterruptFrame {
    pub const BYTE_LEN: usize = FarPointer::BYTE_LEN + size_of::<u16>();
    pub const IP_OFFSET: u32 = 0;
    pub const CS_OFFSET: u32 = FarPointer::SEGMENT_OFFSET;
    pub const FLAGS_OFFSET: u32 = FarPointer::BYTE_LEN as u32;

    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            return_address: FarPointer::from_packed(u32::from_le_bytes(
                bytes[..FarPointer::BYTE_LEN].try_into().unwrap(),
            )),
            flags: u16::from_le_bytes(bytes[FarPointer::BYTE_LEN..].try_into().unwrap()),
        }
    }
}
