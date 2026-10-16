//! References to loaded segment registers, independent of their backing offsets.

use wasm86_compiler::{Val, I1, I32};

use super::Segment;

/// A segment index with its possible registers retained during construction.
/// The private mask is conservative and has no runtime representation.
#[derive(Clone)]
pub(crate) struct SegmentSelection {
    index: Val<I32>,
    possible_segments: u8,
}

impl SegmentSelection {
    /// An internal segment index in encoding order, from zero through five.
    pub(crate) fn indexed(index: Val<I32>) -> Self {
        Self {
            index,
            possible_segments: (1 << Segment::ALL.len()) - 1,
        }
    }

    pub(crate) fn select(
        condition: Val<I1>,
        if_true: impl Into<Self>,
        if_false: impl Into<Self>,
    ) -> Self {
        let if_true = if_true.into();
        let if_false = if_false.into();
        Self {
            index: condition.select(if_true.index, if_false.index),
            possible_segments: if_true.possible_segments | if_false.possible_segments,
        }
    }

    pub(crate) fn index(&self) -> Val<I32> {
        self.index.clone()
    }

    pub(crate) fn possible_segments(&self) -> impl Iterator<Item = Segment> + '_ {
        Segment::ALL
            .into_iter()
            .filter(|segment| self.possible_segments & (1 << *segment as u8) != 0)
    }
}

impl From<Segment> for SegmentSelection {
    fn from(segment: Segment) -> Self {
        Self {
            index: (segment as u32).into(),
            possible_segments: 1 << segment as u8,
        }
    }
}
