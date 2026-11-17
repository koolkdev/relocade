use crate::{
    real_mode::FarPointer, ExecutionProfile, Segment, SegmentDefaultSize, SegmentRegister,
};

/// The origin of bytes supplied for compilation. A captured address retains
/// the loaded CS, including the base and the default used to decode the bytes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CodeAddress {
    /// Raw bytes compiled at an offset under the compiler's fixed profile.
    CsRelative(u32),
    InSegment {
        cs: SegmentRegister,
        eip: u32,
    },
}

impl CodeAddress {
    pub fn eip(self) -> u32 {
        match self {
            Self::CsRelative(eip) | Self::InSegment { eip, .. } => eip,
        }
    }

    pub(super) fn with_eip(self, eip: u32) -> Self {
        match self {
            Self::CsRelative(_) => Self::CsRelative(eip),
            Self::InSegment { cs, .. } => Self::InSegment { cs, eip },
        }
    }

    pub(super) fn code_default(self, profile: ExecutionProfile) -> SegmentDefaultSize {
        match self {
            Self::CsRelative(_) => SegmentDefaultSize::for_profile(profile),
            Self::InSegment { cs, .. } => cs.default_size(),
        }
    }
}

impl From<FarPointer> for CodeAddress {
    fn from(pointer: FarPointer) -> Self {
        Self::InSegment {
            cs: SegmentRegister::real_mode(Segment::Cs, pointer.segment),
            eip: u32::from(pointer.offset),
        }
    }
}
