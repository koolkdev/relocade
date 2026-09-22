//! Supported prefix bytes and the facts collected before form selection.

mod segment;

pub(crate) use segment::SegmentOverride;

use super::OperandSize;
use crate::{
    address::AddressSize,
    segment::{Segment, SegmentDefaultSize},
};

#[derive(Clone, Copy)]
pub(crate) enum Prefix {
    OperandSize,
    AddressSize,
    Group1(Group1Prefix),
    Segment(Segment),
}

/// Supported group-1 prefix bytes; the instruction form determines their meaning.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Group1Prefix {
    F0,
    F2,
    F3,
}

impl Group1Prefix {
    pub(crate) const fn byte(self) -> u8 {
        match self {
            Self::F0 => 0xf0,
            Self::F2 => 0xf2,
            Self::F3 => 0xf3,
        }
    }
}

impl Prefix {
    pub(crate) const ALL: [Self; 11] = [
        Self::OperandSize,
        Self::AddressSize,
        Self::Group1(Group1Prefix::F0),
        Self::Group1(Group1Prefix::F2),
        Self::Group1(Group1Prefix::F3),
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
            Self::AddressSize => 0x67,
            Self::Group1(prefix) => prefix.byte(),
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
/// In particular, F2/F3 become repetition only when a form resolves them that way.
#[derive(Clone)]
pub(crate) struct PrefixState {
    default_size: SegmentDefaultSize,
    operand_size_override: bool,
    address_size_override: bool,
    group1: Option<Group1Prefix>,
    segment_override: SegmentOverride,
}

impl PrefixState {
    pub(crate) fn new(default_size: SegmentDefaultSize) -> Self {
        Self {
            default_size,
            operand_size_override: false,
            address_size_override: false,
            group1: None,
            segment_override: SegmentOverride::None,
        }
    }

    /// Enumerates the independent size and group-1 prefix states for decoder entries.
    /// Override presence selects an entry; its segment identity travels as a value.
    pub(crate) fn combinations(default_size: SegmentDefaultSize) -> impl Iterator<Item = Self> {
        [
            None,
            Some(Group1Prefix::F0),
            Some(Group1Prefix::F2),
            Some(Group1Prefix::F3),
        ]
        .into_iter()
        .flat_map(move |group1| {
            (0..4).map(move |bits| Self {
                operand_size_override: bits & 1 != 0,
                address_size_override: bits & 2 != 0,
                group1,
                ..Self::new(default_size)
            })
        })
    }

    pub(crate) fn with_prefix(mut self, prefix: Prefix) -> Self {
        // Repeated 66/67 preserve presence. For duplicate segment or group-1
        // prefixes, this decoder uses the last one; Intel specifies one per group.
        match prefix {
            Prefix::OperandSize => self.operand_size_override = true,
            Prefix::AddressSize => self.address_size_override = true,
            Prefix::Group1(prefix) => self.group1 = Some(prefix),
            Prefix::Segment(segment) => self.segment_override = SegmentOverride::Fixed(segment),
        }
        self
    }

    pub(crate) fn operand_size(&self) -> OperandSize {
        if (self.default_size == SegmentDefaultSize::Bits16) ^ self.operand_size_override {
            OperandSize::Word
        } else {
            OperandSize::Dword
        }
    }

    pub(crate) fn address_size(&self) -> AddressSize {
        if (self.default_size == SegmentDefaultSize::Bits16) ^ self.address_size_override {
            AddressSize::Bits16
        } else {
            AddressSize::Bits32
        }
    }

    pub(crate) fn group1(&self) -> Option<Group1Prefix> {
        self.group1
    }

    pub(crate) fn segment_override(&self) -> &SegmentOverride {
        &self.segment_override
    }

    pub(crate) fn with_segment_override(mut self, segment_override: SegmentOverride) -> Self {
        self.segment_override = segment_override;
        self
    }

    pub(crate) fn same_form_selection(&self, other: &Self) -> bool {
        self.operand_size() == other.operand_size()
            && self.address_size() == other.address_size()
            && self.group1 == other.group1
    }
}

impl Default for PrefixState {
    fn default() -> Self {
        Self::new(SegmentDefaultSize::Bits32)
    }
}
