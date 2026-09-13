//! A segment override can be fixed by snapshot decoding or carried at runtime.

use wasm86_compiler::{Val, I32};

use crate::segment::{Segment, SegmentSelection};

#[derive(Clone, Default)]
pub(crate) enum SegmentOverride {
    #[default]
    None,
    Fixed(Segment),
    /// An override-bearing decoder entry receives an index from zero through five.
    Runtime(Val<I32>),
}

impl SegmentOverride {
    pub(crate) fn index(&self) -> Option<Val<I32>> {
        match self {
            Self::None => None,
            Self::Fixed(segment) => Some((*segment as u32).into()),
            Self::Runtime(value) => Some(value.clone()),
        }
    }

    pub(crate) fn apply(&self, default: &SegmentSelection) -> SegmentSelection {
        match self {
            Self::None => default.clone(),
            Self::Fixed(segment) => (*segment).into(),
            Self::Runtime(value) => SegmentSelection::Indexed(value.clone()),
        }
    }
}
