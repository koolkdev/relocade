mod binding;
mod constructors;
mod opcodes;

pub(super) use constructors::*;
pub(crate) use opcodes::forms_by_opcode;

use super::{
    handlers::{Handler, SizedHandlers},
    Location, OperandSize,
};
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

/// Physical immediate bytes, independent of the handler's logical operand widths.
#[derive(Clone, Copy)]
pub(crate) enum ImmediateWidth {
    Byte,
    OperandSize,
    SignedByte,
}

impl ImmediateWidth {
    const fn width(self, size: OperandSize) -> FieldWidth {
        match (self, size) {
            (Self::Byte | Self::SignedByte, _) => FieldWidth::Byte,
            (Self::OperandSize, OperandSize::Word) => FieldWidth::Word,
            (Self::OperandSize, OperandSize::Dword) => FieldWidth::Dword,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Encoding {
    OpcodeRegister,
    OpcodeRegisterImmediate {
        immediate: ImmediateWidth,
    },
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
pub(super) enum LocationBinding {
    Register,
    Rm,
    Accumulator,
    AbsoluteOffset,
}

#[derive(Clone, Copy)]
pub(super) enum OperandBinding {
    Location(LocationBinding),
    Immediate,
    /// The r/m address fields form a value; register addressing is not accepted.
    RmAddress,
}

#[derive(Clone, Copy)]
pub(super) enum OperandBindingShape {
    Unary(OperandBinding),
    Binary {
        left: LocationBinding,
        right: OperandBinding,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct Form {
    pub(super) opcode: u8,
    pub(super) mask: u8,
    pub(crate) map: OpcodeMap,
    pub(crate) encoding: Encoding,
    /// Required ModRM.reg opcode extension; otherwise those bits belong to the encoding.
    pub(crate) extension: Option<u8>,
    pub(super) handlers: SizedHandlers<Handler>,
    pub(super) binding: OperandBindingShape,
    pub(super) condition: Option<Condition>,
    pub(super) implicit_memory: bool,
    pub(super) ends_block: bool,
}

impl Form {
    pub(crate) fn with_operand_size(&self, size: OperandSize) -> SizedForm {
        SizedForm {
            form: *self,
            operand_size: size,
            handler: self.handlers.resolve(size),
        }
    }

    /// The caller has already selected this form's opcode map.
    pub(crate) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }

    pub(crate) fn matches_modrm(&self, modrm: u8) -> bool {
        self.extension
            .is_none_or(|extension| ((modrm >> 3) & 7) == extension)
            && (modrm >> 6 != 3 || self.accepts_register_rm())
    }

    pub(crate) fn accepts_register_rm(&self) -> bool {
        !matches!(
            self.binding,
            OperandBindingShape::Unary(OperandBinding::RmAddress)
                | OperandBindingShape::Binary {
                    right: OperandBinding::RmAddress,
                    ..
                }
        )
    }
}

/// Physical fetch widths and the concrete handler are selected from the prefix state.
#[derive(Clone, Copy)]
pub(crate) struct SizedForm {
    form: Form,
    operand_size: OperandSize,
    handler: Handler,
}

impl SizedForm {
    pub(crate) fn encoding(&self) -> Encoding {
        self.form.encoding
    }

    pub(crate) fn immediate_width(&self) -> FieldWidth {
        self.immediate().width(self.operand_size)
    }

    pub(crate) fn sign_extends_immediate(&self) -> bool {
        matches!(self.immediate(), ImmediateWidth::SignedByte)
    }

    fn immediate(&self) -> ImmediateWidth {
        match self.form.encoding {
            Encoding::OpcodeRegisterImmediate { immediate }
            | Encoding::Immediate { immediate }
            | Encoding::RmImmediate { immediate } => immediate,
            _ => unreachable!("the selected encoding contains an immediate field"),
        }
    }
}
