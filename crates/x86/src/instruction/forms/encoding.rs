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

/// Fields preceding the immediate operands, including any complete address.
#[derive(Clone, Copy)]
pub(crate) enum OperandEncoding {
    None,
    OpcodeRegister,
    ModRm,
    /// Address size selects the offset field width independently of data width.
    AbsoluteOffset,
}

/// A form's physical layout. Immediate fields follow the complete operand layout
/// in encoded order, with unused array entries set to None.
#[derive(Clone, Copy)]
pub(crate) struct Encoding {
    pub(crate) operands: OperandEncoding,
    pub(crate) immediates: [Option<ImmediateWidth>; 2],
}

impl Encoding {
    pub(crate) fn has_modrm(self) -> bool {
        matches!(self.operands, OperandEncoding::ModRm)
    }
}

/// Decoders fill the fields required by the form. Binding selects them by role.
pub(crate) struct DecodedFields<V> {
    pub(crate) register: Option<RegisterCode>,
    pub(crate) rm: Option<Location<V>>,
    pub(crate) absolute_offset: Option<V>,
    pub(crate) immediates: [Option<V>; 2],
}

impl<V> Default for DecodedFields<V> {
    fn default() -> Self {
        Self {
            register: None,
            rm: None,
            absolute_offset: None,
            immediates: [None, None],
        }
    }
}
