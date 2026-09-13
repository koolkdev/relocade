//! Supported prefix bytes and the facts collected before form selection.

mod segment;

pub(crate) use segment::SegmentOverride;

use super::OperandSize;
use crate::segment::Segment;

#[derive(Clone, Copy)]
pub(crate) enum Prefix {
    OperandSize,
    F3,
    Segment(Segment),
}

impl Prefix {
    pub(crate) const ALL: [Self; 8] = [
        Self::OperandSize,
        Self::F3,
        Self::Segment(Segment::Es),
        Self::Segment(Segment::Cs),
        Self::Segment(Segment::Ss),
        Self::Segment(Segment::Ds),
        Self::Segment(Segment::Fs),
        Self::Segment(Segment::Gs),
    ];

    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|prefix| prefix.byte() == byte)
    }

    pub(crate) const fn byte(self) -> u8 {
        match self {
            Self::OperandSize => 0x66,
            Self::F3 => 0xf3,
            Self::Segment(segment) => match segment {
                Segment::Es => 0x26,
                Segment::Cs => 0x2e,
                Segment::Ss => 0x36,
                Segment::Ds => 0x3e,
                Segment::Fs => 0x64,
                Segment::Gs => 0x65,
            },
        }
    }
}

/// Prefix presence is separate from its meaning for a particular instruction.
/// In particular, F3 becomes repetition only when a form resolves it that way.
#[derive(Clone, Default)]
pub(crate) struct PrefixState {
    operand_size_override: bool,
    f3: bool,
    segment_override: SegmentOverride,
}

impl PrefixState {
    /// Form selection specializes these facts. Segment overrides travel as
    /// values and do not multiply the decoder's handler variants.
    pub(crate) const PREFIXED: [Self; 3] = [
        Self {
            operand_size_override: true,
            f3: false,
            segment_override: SegmentOverride::None,
        },
        Self {
            operand_size_override: false,
            f3: true,
            segment_override: SegmentOverride::None,
        },
        Self {
            operand_size_override: true,
            f3: true,
            segment_override: SegmentOverride::None,
        },
    ];

    pub(crate) fn with_prefix(mut self, prefix: Prefix) -> Self {
        // Repeated 66/F3 bytes preserve presence; the last segment override wins.
        match prefix {
            Prefix::OperandSize => self.operand_size_override = true,
            Prefix::F3 => self.f3 = true,
            Prefix::Segment(segment) => self.segment_override = SegmentOverride::Fixed(segment),
        }
        self
    }

    pub(crate) fn operand_size(&self) -> OperandSize {
        if self.operand_size_override {
            OperandSize::Word
        } else {
            OperandSize::Dword
        }
    }

    pub(crate) fn has_f3(&self) -> bool {
        self.f3
    }

    pub(crate) fn segment_override(&self) -> &SegmentOverride {
        &self.segment_override
    }

    pub(crate) fn with_segment_override(mut self, segment_override: SegmentOverride) -> Self {
        self.segment_override = segment_override;
        self
    }

    pub(crate) fn same_form_selection(&self, other: &Self) -> bool {
        self.operand_size_override == other.operand_size_override && self.f3 == other.f3
    }
}
