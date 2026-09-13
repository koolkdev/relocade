//! References to loaded segment registers, independent of their backing offsets.

use wasm86_compiler::{Val, I32};

use super::Segment;

#[derive(Clone)]
pub(crate) enum SegmentSelection {
    Named(Segment),
    /// An internal segment index in encoding order, in the range zero through five.
    Indexed(Val<I32>),
}

impl SegmentSelection {
    pub(crate) fn index(&self) -> Val<I32> {
        match self {
            Self::Named(segment) => (*segment as u32).into(),
            Self::Indexed(index) => index.clone(),
        }
    }

    pub(crate) fn known(&self) -> Option<Segment> {
        match self {
            Self::Named(segment) => Some(*segment),
            Self::Indexed(_) => None,
        }
    }
}

impl From<Segment> for SegmentSelection {
    fn from(segment: Segment) -> Self {
        Self::Named(segment)
    }
}
