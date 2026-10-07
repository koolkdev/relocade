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
    /// Canonical ordinary real-mode cache. Zero is a usable segment value.
    /// This is host initialization, not a protected-to-real mode transition.
    pub const fn real_mode(segment: Segment, selector: u16) -> Self {
        Self {
            base: (selector as u32) << 4,
            limit: 0xffff,
            selector,
            attributes: SegmentAttributes::new(
                match segment {
                    Segment::Cs => SegmentKind::Code { readable: true },
                    _ => SegmentKind::Data {
                        writable: true,
                        expand_down: false,
                    },
                },
                SegmentDefaultSize::Bits16,
            ),
        }
    }

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
