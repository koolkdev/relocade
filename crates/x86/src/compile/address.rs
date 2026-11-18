use crate::real_mode::FarPointer;

/// Guest address of a compiled entry. The offset can advance past FFFF during
/// real-mode function discovery; the destination's fetch check handles it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CodeAddress {
    CsRelative(u32),
    RealMode { segment: u16, eip: u32 },
}

impl CodeAddress {
    pub fn eip(self) -> u32 {
        match self {
            Self::CsRelative(eip) | Self::RealMode { eip, .. } => eip,
        }
    }

    pub(super) fn with_eip(self, eip: u32) -> Self {
        match self {
            Self::CsRelative(_) => Self::CsRelative(eip),
            Self::RealMode { segment, .. } => Self::RealMode { segment, eip },
        }
    }
}

impl std::fmt::Display for CodeAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CsRelative(eip) => write!(f, "{eip:x}"),
            Self::RealMode { segment, eip } => write!(f, "{segment:04x}:{eip:04x}"),
        }
    }
}

impl From<FarPointer> for CodeAddress {
    fn from(pointer: FarPointer) -> Self {
        Self::RealMode {
            segment: pointer.segment,
            eip: u32::from(pointer.offset),
        }
    }
}
