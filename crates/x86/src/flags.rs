//! Logical x86 flags, condition dependencies and masked changes.
//! Arithmetic recipes describe status flags; state owns every flag's storage.

mod changes;
mod condition;
mod mask;

pub(crate) use changes::{FlagChange, FlagValues};
pub(crate) use condition::Condition;
pub(crate) use mask::FlagMask;

/// Dense indices for the six flags supplied by arithmetic status sources.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum StatusFlag {
    CF,
    PF,
    AF,
    ZF,
    SF,
    OF,
}

impl StatusFlag {
    pub(crate) const ALL: [Self; 6] = [Self::CF, Self::PF, Self::AF, Self::ZF, Self::SF, Self::OF];

    pub(crate) fn condition(self) -> Option<Condition> {
        Some(match self {
            Self::CF => Condition::B,
            Self::PF => Condition::P,
            Self::AF => return None,
            Self::ZF => Condition::E,
            Self::SF => Condition::S,
            Self::OF => Condition::O,
        })
    }
}

/// Architectural flag identity, independent of its stored representation.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Flag {
    Status(StatusFlag),
    DF,
}

impl Flag {
    pub(crate) const CF: Self = Self::Status(StatusFlag::CF);
    pub(crate) const PF: Self = Self::Status(StatusFlag::PF);
    pub(crate) const AF: Self = Self::Status(StatusFlag::AF);
    pub(crate) const ZF: Self = Self::Status(StatusFlag::ZF);
    pub(crate) const SF: Self = Self::Status(StatusFlag::SF);
    pub(crate) const OF: Self = Self::Status(StatusFlag::OF);
    pub(crate) const ALL: [Self; 7] = [
        Self::CF,
        Self::PF,
        Self::AF,
        Self::ZF,
        Self::SF,
        Self::OF,
        Self::DF,
    ];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Status(flag) => flag as usize,
            Self::DF => StatusFlag::ALL.len(),
        }
    }
}

impl From<StatusFlag> for Flag {
    fn from(flag: StatusFlag) -> Self {
        Self::Status(flag)
    }
}

#[cfg(test)]
mod tests;
