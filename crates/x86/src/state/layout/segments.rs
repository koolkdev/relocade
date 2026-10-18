//! Backing records for visible segment selectors and their loaded caches.

use std::{
    mem::{offset_of, size_of},
    ops::{Index, IndexMut},
};

use super::codec::{read_u16, read_u32, write};

use crate::segment::Segment;

/// A visible selector and the cached properties used for address translation.
/// Descriptor-table edits do not modify this record; a later segment load does.
/// A default record is all zero and unusable. These records describe loaded
/// state; constructing one does not perform descriptor or privilege checks.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoredSegment {
    pub base: u32,
    /// Inclusive byte limit, after applying descriptor granularity.
    pub limit: u32,
    pub selector: u16,
    /// Boolean fields are initialized to 0 or 1. Byte snapshots retain them
    /// verbatim; these are normalized cache properties, not descriptor encodings.
    pub usable: u8,
    pub readable: u8,
    pub writable: u8,
    pub executable: u8,
    pub expand_down: u8,
    /// CS instruction defaults, SS stack width, or the expand-down upper bound.
    pub big: u8,
}

impl StoredSegment {
    pub const BYTE_LEN: usize = size_of::<Self>();

    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        read_segment(&bytes, 0)
    }

    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        write_segment(&mut bytes, 0, self);
        bytes
    }

    /// Canonical ordinary real-mode cache. Zero is a usable segment value.
    /// This is host initialization, not a protected-to-real mode transition.
    pub const fn real_mode(segment: Segment, selector: u16) -> Self {
        Self {
            base: (selector as u32) << 4,
            limit: 0xffff,
            big: 0,
            ..match segment {
                Segment::Cs => Self::flat_code32(selector),
                _ => Self::flat_data32(selector),
            }
        }
    }

    pub const fn flat_code32(selector: u16) -> Self {
        Self {
            writable: 0,
            executable: 1,
            ..Self::flat_data32(selector)
        }
    }

    pub const fn flat_data32(selector: u16) -> Self {
        Self {
            base: 0,
            limit: u32::MAX,
            selector,
            usable: 1,
            readable: 1,
            writable: 1,
            executable: 0,
            expand_down: 0,
            big: 1,
        }
    }

    pub const fn unusable(selector: u16) -> Self {
        Self {
            base: 0,
            limit: 0,
            selector,
            usable: 0,
            readable: 0,
            writable: 0,
            executable: 0,
            expand_down: 0,
            big: 0,
        }
    }
}

/// Six loaded segment registers. A default backing record leaves all unusable;
/// `flat32` provides an explicit host execution configuration.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Segments {
    pub es: StoredSegment,
    pub cs: StoredSegment,
    pub ss: StoredSegment,
    pub ds: StoredSegment,
    pub fs: StoredSegment,
    pub gs: StoredSegment,
}

impl Segments {
    pub const BYTE_LEN: usize = size_of::<Self>();

    pub fn from_bytes(bytes: [u8; Self::BYTE_LEN]) -> Self {
        Self {
            es: read_segment(&bytes, offset_of!(Segments, es)),
            cs: read_segment(&bytes, offset_of!(Segments, cs)),
            ss: read_segment(&bytes, offset_of!(Segments, ss)),
            ds: read_segment(&bytes, offset_of!(Segments, ds)),
            fs: read_segment(&bytes, offset_of!(Segments, fs)),
            gs: read_segment(&bytes, offset_of!(Segments, gs)),
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        for (offset, segment) in [
            (offset_of!(Segments, es), &self.es),
            (offset_of!(Segments, cs), &self.cs),
            (offset_of!(Segments, ss), &self.ss),
            (offset_of!(Segments, ds), &self.ds),
            (offset_of!(Segments, fs), &self.fs),
            (offset_of!(Segments, gs), &self.gs),
        ] {
            write_segment(&mut bytes, offset, segment);
        }
        bytes
    }

    /// Initializes ordinary real-mode caches with zero visible segment values.
    /// The host may replace individual caches with [`StoredSegment::real_mode`].
    pub const fn real_mode() -> Self {
        let data = StoredSegment::real_mode(Segment::Ds, 0);
        Self {
            es: data,
            cs: StoredSegment::real_mode(Segment::Cs, 0),
            ss: data,
            ds: data,
            fs: data,
            gs: data,
        }
    }

    /// Initializes flat caches with zero visible selectors. This host setup is
    /// neither a processor reset image nor a protected-mode segment-load operation.
    pub const fn flat32() -> Self {
        let data = StoredSegment::flat_data32(0);
        Self {
            es: data,
            cs: StoredSegment::flat_code32(0),
            ss: data,
            ds: data,
            fs: data,
            gs: data,
        }
    }
}

impl Index<Segment> for Segments {
    type Output = StoredSegment;

    fn index(&self, segment: Segment) -> &Self::Output {
        match segment {
            Segment::Es => &self.es,
            Segment::Cs => &self.cs,
            Segment::Ss => &self.ss,
            Segment::Ds => &self.ds,
            Segment::Fs => &self.fs,
            Segment::Gs => &self.gs,
        }
    }
}

impl IndexMut<Segment> for Segments {
    fn index_mut(&mut self, segment: Segment) -> &mut Self::Output {
        match segment {
            Segment::Es => &mut self.es,
            Segment::Cs => &mut self.cs,
            Segment::Ss => &mut self.ss,
            Segment::Ds => &mut self.ds,
            Segment::Fs => &mut self.fs,
            Segment::Gs => &mut self.gs,
        }
    }
}

fn read_segment(bytes: &[u8], offset: usize) -> StoredSegment {
    StoredSegment {
        base: read_u32(bytes, offset + offset_of!(StoredSegment, base)),
        limit: read_u32(bytes, offset + offset_of!(StoredSegment, limit)),
        selector: read_u16(bytes, offset + offset_of!(StoredSegment, selector)),
        usable: bytes[offset + offset_of!(StoredSegment, usable)],
        readable: bytes[offset + offset_of!(StoredSegment, readable)],
        writable: bytes[offset + offset_of!(StoredSegment, writable)],
        executable: bytes[offset + offset_of!(StoredSegment, executable)],
        expand_down: bytes[offset + offset_of!(StoredSegment, expand_down)],
        big: bytes[offset + offset_of!(StoredSegment, big)],
    }
}

fn write_segment(bytes: &mut [u8], offset: usize, segment: &StoredSegment) {
    write(
        bytes,
        offset + offset_of!(StoredSegment, base),
        &segment.base.to_le_bytes(),
    );
    write(
        bytes,
        offset + offset_of!(StoredSegment, limit),
        &segment.limit.to_le_bytes(),
    );
    write(
        bytes,
        offset + offset_of!(StoredSegment, selector),
        &segment.selector.to_le_bytes(),
    );
    bytes[offset + offset_of!(StoredSegment, usable)] = segment.usable;
    bytes[offset + offset_of!(StoredSegment, readable)] = segment.readable;
    bytes[offset + offset_of!(StoredSegment, writable)] = segment.writable;
    bytes[offset + offset_of!(StoredSegment, executable)] = segment.executable;
    bytes[offset + offset_of!(StoredSegment, expand_down)] = segment.expand_down;
    bytes[offset + offset_of!(StoredSegment, big)] = segment.big;
}
