//! Physical instruction fields, before binding them to semantic operands.

use crate::{
    instruction::{Location, OperandSize},
    register::RegisterCode,
};

/// Storage width of an integer field in the instruction encoding.
#[derive(Clone, Copy)]
pub(crate) enum FieldWidth {
    Byte,
    Word,
    Dword,
}

impl FieldWidth {
    pub(crate) const fn bytes(self) -> u32 {
        match self {
            Self::Byte => 1,
            Self::Word => 2,
            Self::Dword => 4,
        }
    }
}

/// Physical immediate bytes, independent of a handler's logical operand widths.
#[derive(Clone, Copy)]
pub(crate) enum ImmediateWidth {
    Byte,
    Word,
    OperandSize,
    SignedByte,
}

impl ImmediateWidth {
    pub(super) const fn width(self, size: OperandSize) -> FieldWidth {
        match (self, size) {
            (Self::Byte | Self::SignedByte, _) => FieldWidth::Byte,
            (Self::Word, _) => FieldWidth::Word,
            (Self::OperandSize, OperandSize::Word) => FieldWidth::Word,
            (Self::OperandSize, OperandSize::Dword) => FieldWidth::Dword,
        }
    }

    pub(crate) const fn is_signed(self) -> bool {
        matches!(self, Self::SignedByte)
    }
}

/// Immediate fields in their encoded order. Widths and decoded values use the
/// same shape; a failed read prevents reading any later field.
#[derive(Clone, Copy)]
pub(crate) enum ImmediateFields<T> {
    One(T),
    Two(T, T),
}

impl<T> ImmediateFields<T> {
    pub(crate) const fn len(&self) -> usize {
        match self {
            Self::One(_) => 1,
            Self::Two(_, _) => 2,
        }
    }

    pub(crate) fn get(&self, index: usize) -> &T {
        match (self, index) {
            (Self::One(first) | Self::Two(first, _), 0) => first,
            (Self::Two(_, second), 1) => second,
            _ => panic!("the binding selects an encoded immediate field"),
        }
    }

    pub(crate) fn try_map<U, E>(
        self,
        mut map: impl FnMut(T) -> Result<U, E>,
    ) -> Result<ImmediateFields<U>, E> {
        Ok(match self {
            Self::One(first) => ImmediateFields::One(map(first)?),
            Self::Two(first, second) => {
                let first = map(first)?;
                let second = map(second)?;
                ImmediateFields::Two(first, second)
            }
        })
    }
}

impl ImmediateFields<ImmediateWidth> {
    pub(super) const fn append(self, width: ImmediateWidth) -> Self {
        match self {
            Self::One(first) => Self::Two(first, width),
            Self::Two(_, _) => panic!("a form has at most two immediate fields"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Encoding {
    OpcodeOnly,
    OpcodeRegister,
    OpcodeRegisterImmediate {
        immediates: ImmediateFields<ImmediateWidth>,
    },
    Immediate {
        immediates: ImmediateFields<ImmediateWidth>,
    },
    /// ModRM supplies reg/rm fields. Immediates follow the complete address.
    ModRm {
        immediates: Option<ImmediateFields<ImmediateWidth>>,
    },
    /// Address size selects the offset field width independently of data width.
    AccumulatorOffset,
}

impl Encoding {
    pub(crate) fn has_modrm(self) -> bool {
        matches!(self, Self::ModRm { .. })
    }

    pub(crate) const fn immediates(self) -> Option<ImmediateFields<ImmediateWidth>> {
        match self {
            Self::OpcodeRegisterImmediate { immediates } | Self::Immediate { immediates } => {
                Some(immediates)
            }
            Self::ModRm { immediates } => immediates,
            _ => None,
        }
    }
}

pub(crate) enum DecodedFields<V> {
    OpcodeOnly,
    OpcodeRegister {
        register: RegisterCode,
    },
    OpcodeRegisterImmediate {
        register: RegisterCode,
        immediates: ImmediateFields<V>,
    },
    Immediate {
        immediates: ImmediateFields<V>,
    },
    ModRm {
        register: RegisterCode,
        rm: Location<V>,
        immediates: Option<ImmediateFields<V>>,
    },
    AccumulatorOffset {
        offset: V,
    },
}
