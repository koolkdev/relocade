//! Backing records for visible segment selectors and their loaded caches.

use std::ops::{Index, IndexMut};

use crate::segment::{Segment, SegmentAttributes, SegmentDefaultSize, SegmentKind};

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
    pub attributes: SegmentAttributes,
}

impl StoredSegment {
    pub const fn flat_code32(selector: u16) -> Self {
        Self {
            base: 0,
            limit: u32::MAX,
            selector,
            attributes: SegmentAttributes::new(
                SegmentKind::Code { readable: true },
                SegmentDefaultSize::Bits32,
            ),
        }
    }

    pub const fn flat_data32(selector: u16) -> Self {
        Self {
            base: 0,
            limit: u32::MAX,
            selector,
            attributes: SegmentAttributes::new(
                SegmentKind::Data {
                    writable: true,
                    expand_down: false,
                },
                SegmentDefaultSize::Bits32,
            ),
        }
    }

    pub const fn unusable(selector: u16) -> Self {
        Self {
            base: 0,
            limit: 0,
            selector,
            attributes: SegmentAttributes::unusable(),
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
