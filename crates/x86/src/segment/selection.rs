//! References to loaded segment registers, independent of their backing offsets.

use wasm86_compiler::{Val, I1, I32};

use super::Segment;

/// A segment index with a hint for the address decoder's DS/SS default.
#[derive(Clone)]
pub(crate) struct SegmentSelection {
    index: Val<I32>,
    ds_or_ss: bool,
}

impl SegmentSelection {
    /// An internal segment index in encoding order, from zero through five.
    pub(crate) fn indexed(index: Val<I32>) -> Self {
        Self {
            index,
            ds_or_ss: false,
        }
    }

    pub(crate) fn ds_or_ss(stack_base: Val<I1>) -> Self {
        Self {
            index: stack_base.select(Segment::Ss as u32, Segment::Ds as u32),
            ds_or_ss: true,
        }
    }

    pub(crate) fn index(&self) -> Val<I32> {
        self.index.clone()
    }

    pub(crate) fn is_ds_or_ss(&self) -> bool {
        self.ds_or_ss
    }
}

impl From<Segment> for SegmentSelection {
    fn from(segment: Segment) -> Self {
        Self::indexed((segment as u32).into())
    }
}
