use crate::real_mode::FarPointer;

/// Guest address of a compiled entry. A real-mode EIP can advance beyond FFFF.
/// The destination checks whether its complete fetch span fits CS.
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

    /// The real-mode CS selector, when this address names a segment explicitly.
    pub fn segment(self) -> Option<u16> {
        match self {
            Self::CsRelative(_) => None,
            Self::RealMode { segment, .. } => Some(segment),
        }
    }

    /// An offset in the same code segment as this address.
    pub fn with_eip(self, eip: u32) -> Self {
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
