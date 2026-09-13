//! A segment override can be fixed by snapshot decoding or carried at runtime.

use wasm86_compiler::{Val, I32};

use crate::segment::{Segment, SegmentSelection};

#[derive(Clone, Default)]
pub(crate) enum SegmentOverride {
    #[default]
    None,
    Fixed(Segment),
    /// Decoder transport uses zero through five for a segment and six for none.
    Runtime(Val<I32>),
}

impl SegmentOverride {
    const ABSENT: u32 = 6;

    pub(crate) fn encoded(&self) -> Val<I32> {
        match self {
            Self::None => Self::ABSENT.into(),
            Self::Fixed(segment) => (*segment as u32).into(),
            Self::Runtime(value) => value.clone(),
        }
    }

    pub(crate) fn apply(&self, default: &SegmentSelection) -> SegmentSelection {
        match self {
            Self::None => default.clone(),
            Self::Fixed(segment) => (*segment).into(),
            Self::Runtime(value) => {
                SegmentSelection::Indexed(value.eq(Self::ABSENT).select(default.index(), value))
            }
        }
    }
}
