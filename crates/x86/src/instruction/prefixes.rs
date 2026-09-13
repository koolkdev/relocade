//! Supported prefix bytes and the facts collected before form selection.

use super::OperandSize;

#[derive(Clone, Copy)]
pub(crate) enum Prefix {
    OperandSize,
    F3,
}

impl Prefix {
    pub(crate) const ALL: [Self; 2] = [Self::OperandSize, Self::F3];

    pub(crate) fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|prefix| prefix.byte() == byte)
    }

    pub(crate) const fn byte(self) -> u8 {
        match self {
            Self::OperandSize => 0x66,
            Self::F3 => 0xf3,
        }
    }
}

/// Prefix presence is separate from its meaning for a particular instruction.
/// In particular, F3 becomes repetition only when a form resolves it that way.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct PrefixState {
    operand_size_override: bool,
    f3: bool,
}

impl PrefixState {
    /// Distinct states reached by the supported prefixes. Runtime decoders only
    /// specialize the states accepted by forms at their entry point.
    pub(crate) const PREFIXED: [Self; 3] = [
        Self {
            operand_size_override: true,
            f3: false,
        },
        Self {
            operand_size_override: false,
            f3: true,
        },
        Self {
            operand_size_override: true,
            f3: true,
        },
    ];

    pub(crate) fn with_prefix(mut self, prefix: Prefix) -> Self {
        // Repeated copies preserve presence; an operand-size override never toggles.
        match prefix {
            Prefix::OperandSize => self.operand_size_override = true,
            Prefix::F3 => self.f3 = true,
        }
        self
    }

    pub(crate) fn operand_size(self) -> OperandSize {
        if self.operand_size_override {
            OperandSize::Word
        } else {
            OperandSize::Dword
        }
    }

    pub(crate) fn has_f3(self) -> bool {
        self.f3
    }
}
