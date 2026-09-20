//! Segment identities, loaded caches, and host descriptor-table resolution.

mod access;
mod descriptor;
mod profile;
mod selection;
mod tables;

use wasm86_compiler::{Val, I16, I32};

pub(crate) use access::SegmentAccess;
pub use descriptor::{
    PrivilegeLevel, SegmentDescriptor, SegmentDescriptorKind, SegmentPermissions,
};
pub use profile::SegmentProfile;
pub(crate) use selection::SegmentSelection;
pub use tables::DescriptorTables;

/// Symbolic values of a loaded segment record. The selector does not determine access.
pub(crate) struct SegmentValues {
    pub(crate) base: Val<I32>,
    pub(crate) limit: Val<I32>,
    pub(crate) selector: Val<I16>,
    pub(crate) attributes: Val<I16>,
}

#[cfg(test)]
mod tests;

/// Segment registers in x86 encoding order, independent of their backing offsets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Segment {
    Es,
    Cs,
    Ss,
    Ds,
    Fs,
    Gs,
}

impl Segment {
    pub const ALL: [Self; 6] = [Self::Es, Self::Cs, Self::Ss, Self::Ds, Self::Fs, Self::Gs];
}

/// The D/B attribute, independent of a segment's base and effective limit.
/// It selects CS instruction defaults, SS stack-pointer width, or the upper
/// bound of an expand-down data segment. It does not size an ordinary data segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentDefaultSize {
    Bits16,
    Bits32,
}

/// Access properties of a usable code or data segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentKind {
    Data { writable: bool, expand_down: bool },
    Code { readable: bool },
}

/// Normalized cache attributes, not the bit encoding of a GDT/LDT descriptor.
/// Bits 0 through 4 mean usable, code, readable-code/writable-data, expand-down,
/// and D/B, respectively. Bits 5 through 15 are reserved and retained verbatim.
/// Usability is stored independently of the visible selector.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SegmentAttributes(u16);

impl SegmentAttributes {
    const USABLE: u16 = 1;
    const CODE: u16 = 1 << 1;
    const READ_WRITE: u16 = 1 << 2;
    const EXPAND_DOWN: u16 = 1 << 3;
    const DEFAULT_BIG: u16 = 1 << 4;

    /// Constructs a usable cache's attributes without performing a segment load.
    pub const fn new(kind: SegmentKind, default_size: SegmentDefaultSize) -> Self {
        let kind = match kind {
            SegmentKind::Data {
                writable,
                expand_down,
            } => {
                ((writable as u16) * Self::READ_WRITE) | ((expand_down as u16) * Self::EXPAND_DOWN)
            }
            SegmentKind::Code { readable } => Self::CODE | ((readable as u16) * Self::READ_WRITE),
        };
        let size = match default_size {
            SegmentDefaultSize::Bits16 => 0,
            SegmentDefaultSize::Bits32 => Self::DEFAULT_BIG,
        };
        Self(Self::USABLE | kind | size)
    }

    pub const fn unusable() -> Self {
        Self(0)
    }

    /// Reads a backing value without validating or normalizing any bit.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn is_usable(self) -> bool {
        self.0 & Self::USABLE != 0
    }

    /// Returns the usable type. An unusable cache or the invalid combination
    /// of code and expand-down attributes has no usable type.
    pub const fn kind(self) -> Option<SegmentKind> {
        if !self.is_usable() {
            return None;
        }
        let access = self.0 & Self::READ_WRITE != 0;
        let expand_down = self.0 & Self::EXPAND_DOWN != 0;
        if self.0 & Self::CODE != 0 {
            if expand_down {
                None
            } else {
                Some(SegmentKind::Code { readable: access })
            }
        } else {
            Some(SegmentKind::Data {
                writable: access,
                expand_down,
            })
        }
    }

    pub const fn default_size(self) -> SegmentDefaultSize {
        if self.0 & Self::DEFAULT_BIG == 0 {
            SegmentDefaultSize::Bits16
        } else {
            SegmentDefaultSize::Bits32
        }
    }
}
