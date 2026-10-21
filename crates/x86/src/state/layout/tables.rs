//! Architectural table registers and the retained LDTR/TR descriptor caches.

use super::codec::{read, read_u16, read_u32, write};
use std::mem::{offset_of, size_of};

/// GDTR or IDTR backing. The limit is the inclusive byte offset from the base.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DescriptorTableRegister {
    pub base: u32,
    pub limit: u16,
    pub reserved: [u8; 2],
}

impl DescriptorTableRegister {
    pub const BYTE_LEN: usize = size_of::<Self>();

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
            base: read_u32(&bytes, offset_of!(Self, base)),
            limit: read_u16(&bytes, offset_of!(Self, limit)),
            reserved: read(&bytes, offset_of!(Self, reserved)),
        }
    }

    pub fn to_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write(&mut bytes, offset_of!(Self, base), &self.base.to_le_bytes());
        write(
            &mut bytes,
            offset_of!(Self, limit),
            &self.limit.to_le_bytes(),
        );
        write(&mut bytes, offset_of!(Self, reserved), &self.reserved);
        bytes
    }
}

/// Visible LDTR/TR selector and its loaded descriptor cache. Editing table
/// memory does not update this cache. An unusable LDTR disables LDT lookup.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemSegmentRegister {
    pub base: u32,
    /// Inclusive effective byte limit, after applying descriptor granularity.
    pub limit: u32,
    pub selector: u16,
    pub usable: u8,
    pub reserved: u8,
    /// Descriptor attributes in LAR result positions.
    pub access_rights: u32,
}

impl SystemSegmentRegister {
    pub const BYTE_LEN: usize = size_of::<Self>();

    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            base: read_u32(&bytes, offset_of!(Self, base)),
            limit: read_u32(&bytes, offset_of!(Self, limit)),
            selector: read_u16(&bytes, offset_of!(Self, selector)),
            usable: bytes[offset_of!(Self, usable)],
            reserved: bytes[offset_of!(Self, reserved)],
            access_rights: read_u32(&bytes, offset_of!(Self, access_rights)),
        }
    }

    pub fn to_bytes(self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write(&mut bytes, offset_of!(Self, base), &self.base.to_le_bytes());
        write(
            &mut bytes,
            offset_of!(Self, limit),
            &self.limit.to_le_bytes(),
        );
        write(
            &mut bytes,
            offset_of!(Self, selector),
            &self.selector.to_le_bytes(),
        );
        bytes[offset_of!(Self, usable)] = self.usable;
        bytes[offset_of!(Self, reserved)] = self.reserved;
        write(
            &mut bytes,
            offset_of!(Self, access_rights),
            &self.access_rights.to_le_bytes(),
        );
        bytes
    }
}
