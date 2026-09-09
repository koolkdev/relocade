mod binding;
mod catalog;
mod opcodes;

pub(crate) use catalog::*;
pub(crate) use opcodes::forms_by_opcode;

use super::{BinaryOperation, Location, OperandSize, OperandWidth, UnaryOperation};
use crate::{flags::Condition, register::RegisterCode};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum OpcodeMap {
    Primary,
    Extended,
}

impl OpcodeMap {
    pub(crate) const fn bytes(self) -> u32 {
        match self {
            Self::Primary => 1,
            Self::Extended => 2,
        }
    }
}

/// Physical immediate width and its conversion to the instruction operand width.
#[derive(Clone, Copy)]
pub(crate) enum ImmediateWidth {
    Operand,
    SignedByte,
}

#[derive(Clone, Copy)]
pub(crate) enum Encoding {
    OpcodeRegister,
    OpcodeRegisterImmediate,
    Immediate {
        immediate: ImmediateWidth,
    },
    RegisterRm,
    /// The immediate follows any address fields.
    RmImmediate {
        immediate: ImmediateWidth,
    },
    /// The r/m field names the only operand. The form may constrain ModRM.reg.
    Rm,
    /// The address field remains 32-bit regardless of the data width.
    AccumulatorOffset,
}

impl Encoding {
    pub(crate) fn has_modrm(self) -> bool {
        matches!(self, Self::RegisterRm | Self::RmImmediate { .. } | Self::Rm)
    }
}

#[derive(Clone, Copy)]
enum WidthRule {
    Byte,
    OperandSize,
}
impl WidthRule {
    const fn resolve(self, size: OperandSize) -> OperandWidth {
        match (self, size) {
            (Self::Byte, _) => OperandWidth::Byte,
            (Self::OperandSize, OperandSize::Word) => OperandWidth::Word,
            (Self::OperandSize, OperandSize::Dword) => OperandWidth::Dword,
        }
    }
}

/// Physical fields, before assignment to the operation's operand roles.
pub(crate) enum DecodedFields<V> {
    Location(Location<V>),
    OpcodeRegisterImmediate {
        register: RegisterCode,
        immediate: V,
    },
    Immediate {
        immediate: V,
    },
    RegisterRm {
        register: RegisterCode,
        rm: Location<V>,
    },
    RmImmediate {
        rm: Location<V>,
        immediate: V,
    },
    AccumulatorOffset {
        offset: V,
    },
}

/// A location's role is independent of the fields needed to decode it.
#[derive(Clone, Copy)]
enum LocationBinding {
    Register,
    Rm,
    Accumulator,
    AbsoluteOffset,
}

#[derive(Clone, Copy)]
enum OperandBinding {
    Location(LocationBinding),
    Immediate,
}

#[derive(Clone, Copy)]
enum Operation {
    Binary {
        operation: BinaryOperation,
        left: LocationBinding,
        right: OperandBinding,
    },
    Unary(UnaryOperation),
    Push(OperandBinding),
    Pop(LocationBinding),
    SetCondition(Condition),
}

#[derive(Clone, Copy)]
pub(crate) struct Form {
    opcode: u8,
    mask: u8,
    pub(crate) map: OpcodeMap,
    pub(crate) encoding: Encoding,
    /// Required ModRM.reg opcode extension; otherwise those bits belong to the encoding.
    pub(crate) extension: Option<u8>,
    width: WidthRule,
    operation: Operation,
}
impl Form {
    pub(crate) const fn with_operand_size(&self, size: OperandSize) -> SizedForm {
        SizedForm {
            encoding: self.encoding,
            width: self.width.resolve(size),
            operation: self.operation,
        }
    }
    /// The caller has already selected this form's opcode map.
    pub(crate) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }
    pub(crate) fn matches_modrm(&self, modrm: u8) -> bool {
        match self.extension {
            Some(extension) => ((modrm >> 3) & 7) == extension,
            None => true,
        }
    }
}

/// A form whose operand width is fixed before its fields are read.
#[derive(Clone, Copy)]
pub(crate) struct SizedForm {
    pub(crate) encoding: Encoding,
    pub(crate) width: OperandWidth,
    operation: Operation,
}
impl SizedForm {
    pub(crate) const fn immediate_width(&self) -> OperandWidth {
        match self.encoding {
            Encoding::Immediate {
                immediate: ImmediateWidth::SignedByte,
            }
            | Encoding::RmImmediate {
                immediate: ImmediateWidth::SignedByte,
            } => OperandWidth::Byte,
            _ => self.width,
        }
    }
    pub(crate) fn sign_extends_immediate(&self) -> bool {
        matches!(
            self.encoding,
            Encoding::Immediate {
                immediate: ImmediateWidth::SignedByte,
            } | Encoding::RmImmediate {
                immediate: ImmediateWidth::SignedByte,
            }
        )
    }
}
