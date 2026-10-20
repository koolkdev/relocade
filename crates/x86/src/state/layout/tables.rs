//! Architectural table registers and the retained LDTR/TR descriptor caches.

use super::codec::{read, read_u16, read_u32, write};

/// GDTR or IDTR backing. The limit is the inclusive byte offset from the base.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredTableRegister {
    pub base: u32,
    pub limit: u16,
    pub reserved: [u8; 2],
}

impl StoredTableRegister {
    pub const BYTE_LEN: usize = 8;

    pub const fn new(base: u32, limit: u16) -> Self {
        Self {
            base,
            limit,
            reserved: [0; 2],
        }
    }

    /// The interrupt vector table used by the real-mode runtime.
    pub const REAL_MODE_IDTR: Self = Self::new(0, 0x3ff);

    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            base: read_u32(&bytes, 0),
            limit: read_u16(&bytes, 4),
            reserved: read(&bytes, 6),
        }
    }

    pub fn to_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write(&mut bytes, 0, &self.base.to_le_bytes());
        write(&mut bytes, 4, &self.limit.to_le_bytes());
        write(&mut bytes, 6, &self.reserved);
        bytes
    }
}

/// Visible LDTR/TR selector and its loaded descriptor cache. Editing table
/// memory does not update this cache. An unusable LDTR disables LDT lookup.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredSystemSegment {
    pub base: u32,
    /// Inclusive effective byte limit, after applying descriptor granularity.
    pub limit: u32,
    pub selector: u16,
    pub usable: u8,
    pub reserved: u8,
    /// Descriptor attributes in LAR result positions.
    pub access_rights: u32,
}

impl StoredSystemSegment {
    pub const BYTE_LEN: usize = 16;

    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            base: read_u32(&bytes, 0),
            limit: read_u32(&bytes, 4),
            selector: read_u16(&bytes, 8),
            usable: bytes[10],
            reserved: bytes[11],
            access_rights: read_u32(&bytes, 12),
        }
    }

    pub fn to_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write(&mut bytes, 0, &self.base.to_le_bytes());
        write(&mut bytes, 4, &self.limit.to_le_bytes());
        write(&mut bytes, 8, &self.selector.to_le_bytes());
        bytes[10] = self.usable;
        bytes[11] = self.reserved;
        write(&mut bytes, 12, &self.access_rights.to_le_bytes());
        bytes
    }
}
