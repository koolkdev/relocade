//! Deferred flag descriptions, partial changes and condition queries.
//! These values describe flags; ALU operations construct destination results.

mod changes;
mod condition;
mod source;

pub(crate) use changes::{FlagChange, FlagMask, FlagValues};
pub(crate) use condition::Condition;
pub(crate) use source::{AnyFlagSource, FlagSource};

/// Dense indices for symbolic flag values; CPU record offsets belong to state.
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
}

#[cfg(test)]
mod tests;
