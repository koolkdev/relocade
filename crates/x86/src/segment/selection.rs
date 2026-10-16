//! References to loaded segment registers, independent of their backing offsets.

use wasm86_compiler::{Val, I32};

use super::Segment;

/// A named segment or an index computed by address decoding or an override.
#[derive(Clone)]
pub(crate) enum SegmentSelection {
    Named(Segment),
    /// An internal segment index in encoding order, from zero through five.
    Indexed(Val<I32>),
}

impl SegmentSelection {
    pub(crate) fn index(&self) -> Val<I32> {
        match self {
            Self::Named(segment) => (*segment as u32).into(),
            Self::Indexed(index) => index.clone(),
        }
    }
}

impl From<Segment> for SegmentSelection {
    fn from(segment: Segment) -> Self {
        Self::Named(segment)
    }
}
