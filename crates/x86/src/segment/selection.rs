//! References to loaded segment registers, independent of their backing offsets.

use wasm86_compiler::{Val, I32};

use super::Segment;

/// A named segment, the architectural address default, or an explicit runtime
/// override. Keeping the address default distinct lets flat entries omit its
/// DS/SS choice while segmented entries resolve the actual segment.
#[derive(Clone)]
pub(crate) enum SegmentSelection {
    Named(Segment),
    /// Address construction supplies the index of DS or SS.
    AddressDefault(Val<I32>),
    /// An internal segment index in encoding order, from zero through five.
    Indexed(Val<I32>),
}

impl SegmentSelection {
    pub(crate) fn index(&self) -> Val<I32> {
        match self {
            Self::Named(segment) => (*segment as u32).into(),
            Self::AddressDefault(index) | Self::Indexed(index) => index.clone(),
        }
    }
}

impl From<Segment> for SegmentSelection {
    fn from(segment: Segment) -> Self {
        Self::Named(segment)
    }
}
