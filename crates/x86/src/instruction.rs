use wasm86_compiler::{Val, I1, I8};

use crate::{address::Address32, register::RegisterCode};

pub(super) const MAX_INSTRUCTION_BYTES: u32 = 15;
pub(super) const OPERAND_SIZE_PREFIX: u8 = 0x66;

#[derive(Clone, Copy)]
pub(super) enum Semantic {
    Mov,
}

#[derive(Clone, Copy)]
pub(super) enum Encoding {
    OpcodeRegisterImmediate,
    RegisterRm {
        register: RegisterRole,
    },
    /// ModRM.reg selects the opcode extension, while r/m names the destination.
    RmImmediate {
        extension: u8,
    },
    /// The offset field remains 32-bit regardless of the data operand width.
    AccumulatorOffset {
        accumulator: RegisterRole,
    },
}

impl Encoding {
    /// Every supported format has one unprefixed opcode byte.
    pub(super) const OPCODE_BYTES: u32 = 1;

    /// Tests an opcode extension after the opcode has selected this form.
    pub(super) fn matches_modrm(self, modrm: u8) -> bool {
        match self {
            Encoding::RmImmediate { extension } => ((modrm >> 3) & 7) == extension,
            _ => true,
        }
    }

    /// Returns a rejection predicate only for an encoding with an opcode extension.
    pub(super) fn extension_mismatch(self, modrm: &Val<I8>) -> Option<Val<I1>> {
        match self {
            Encoding::RmImmediate { extension } => {
                Some(modrm.and(0x38).ne(u32::from(extension) << 3))
            }
            _ => None,
        }
    }
}

/// Width of the instruction's data operands; effective addresses remain 32-bit.
#[derive(Clone, Copy)]
pub(super) enum OperandWidth {
    Byte,
    Word,
    Dword,
}

impl OperandWidth {
    pub(super) const fn bytes(self) -> u32 {
        match self {
            Self::Byte => 1,
            Self::Word => 2,
            Self::Dword => 4,
        }
    }
}

/// The effective operand-size attribute in the supported default-32 mode.
#[derive(Clone, Copy)]
pub(super) enum OperandSize {
    Word,
    Dword,
}

#[derive(Clone, Copy)]
enum WidthRule {
    Byte,
    OperandSize,
}

impl WidthRule {
    const fn resolve(self, operand_size: OperandSize) -> OperandWidth {
        match (self, operand_size) {
            (Self::Byte, _) => OperandWidth::Byte,
            (Self::OperandSize, OperandSize::Word) => OperandWidth::Word,
            (Self::OperandSize, OperandSize::Dword) => OperandWidth::Dword,
        }
    }
}

/// Role of the ModRM.reg field or implicit accumulator in the instruction.
#[derive(Clone, Copy)]
pub(super) enum RegisterRole {
    Destination,
    Source,
}

impl RegisterRole {
    fn bind<V>(self, register: RegisterCode, other: Location<V>) -> (Location<V>, Operand<V>) {
        let register = Location::Register(register);
        match self {
            Self::Destination => (register, Operand::Location(other)),
            Self::Source => (other, Operand::Location(register)),
        }
    }
}

/// Decoded fields follow the physical layout, before assignment to semantic roles.
pub(super) enum DecodedFields<V> {
    OpcodeRegisterImmediate {
        register: RegisterCode,
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

pub(super) struct Form {
    opcode: u8,
    mask: u8,
    pub(super) encoding: Encoding,
    width: WidthRule,
    semantic: Semantic,
}

impl Form {
    pub(super) const fn resolve(&self, operand_size: OperandSize) -> ResolvedForm {
        ResolvedForm {
            encoding: self.encoding,
            width: self.width.resolve(operand_size),
            semantic: self.semantic,
        }
    }

    pub(super) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }

    pub(super) fn matches_value(&self, opcode: &Val<I8>) -> Val<I1> {
        opcode.and(u32::from(self.mask)).eq(u32::from(self.opcode))
    }
}

/// A selected form with its data width fixed before operand fields are read.
#[derive(Clone, Copy)]
pub(super) struct ResolvedForm {
    pub(super) encoding: Encoding,
    pub(super) width: OperandWidth,
    semantic: Semantic,
}

impl ResolvedForm {
    /// Opcode and shortest operand fields, excluding any prefixes.
    pub(super) const fn minimum_length(&self) -> u32 {
        Encoding::OPCODE_BYTES
            + match self.encoding {
                Encoding::OpcodeRegisterImmediate => self.width.bytes(),
                Encoding::RegisterRm { .. } => 1,
                Encoding::RmImmediate { .. } => 1 + self.width.bytes(),
                Encoding::AccumulatorOffset { .. } => 4,
            }
    }

    /// Binds fields decoded according to this form's encoding.
    pub(super) fn bind<V, P>(
        &self,
        fields: DecodedFields<V>,
        eip: P,
        next_eip: P,
    ) -> DecodedInstruction<V, P> {
        let (destination, source) = match (self.encoding, fields) {
            (
                Encoding::OpcodeRegisterImmediate,
                DecodedFields::OpcodeRegisterImmediate {
                    register,
                    immediate,
                },
            ) => (Location::Register(register), Operand::Immediate(immediate)),
            (
                Encoding::RegisterRm { register: role },
                DecodedFields::RegisterRm { register, rm },
            ) => role.bind(register, rm),
            (Encoding::RmImmediate { .. }, DecodedFields::RmImmediate { rm, immediate }) => {
                (rm, Operand::Immediate(immediate))
            }
            (
                Encoding::AccumulatorOffset { accumulator: role },
                DecodedFields::AccumulatorOffset { offset },
            ) => role.bind(
                RegisterCode::from_code(0),
                Location::Memory(Address32 {
                    base: None,
                    index: None,
                    displacement: offset,
                }),
            ),
            _ => unreachable!("decoded fields match the selected encoding"),
        };
        DecodedInstruction {
            instruction: Instruction {
                semantic: self.semantic,
                width: self.width,
                destination,
                source,
            },
            eip,
            next_eip,
        }
    }
}

pub(super) const MOV_OPERAND_IMMEDIATE: Form = Form {
    opcode: 0xb8,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    width: WidthRule::OperandSize,
    semantic: Semantic::Mov,
};

pub(super) const MOV_BYTE_IMMEDIATE: Form = Form {
    opcode: 0xb0,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    width: WidthRule::Byte,
    semantic: Semantic::Mov,
};

pub(super) const OPCODE_REGISTER_IMMEDIATE_FORMS: [Form; 2] =
    [MOV_OPERAND_IMMEDIATE, MOV_BYTE_IMMEDIATE];

pub(super) const MODRM_FORMS: [Form; 6] = [
    Form {
        opcode: 0x89,
        mask: 0xff,
        encoding: Encoding::RegisterRm {
            register: RegisterRole::Source,
        },
        width: WidthRule::OperandSize,
        semantic: Semantic::Mov,
    },
    Form {
        opcode: 0x8b,
        mask: 0xff,
        encoding: Encoding::RegisterRm {
            register: RegisterRole::Destination,
        },
        width: WidthRule::OperandSize,
        semantic: Semantic::Mov,
    },
    Form {
        opcode: 0x88,
        mask: 0xff,
        encoding: Encoding::RegisterRm {
            register: RegisterRole::Source,
        },
        width: WidthRule::Byte,
        semantic: Semantic::Mov,
    },
    Form {
        opcode: 0x8a,
        mask: 0xff,
        encoding: Encoding::RegisterRm {
            register: RegisterRole::Destination,
        },
        width: WidthRule::Byte,
        semantic: Semantic::Mov,
    },
    Form {
        opcode: 0xc6,
        mask: 0xff,
        encoding: Encoding::RmImmediate { extension: 0 },
        width: WidthRule::Byte,
        semantic: Semantic::Mov,
    },
    Form {
        opcode: 0xc7,
        mask: 0xff,
        encoding: Encoding::RmImmediate { extension: 0 },
        width: WidthRule::OperandSize,
        semantic: Semantic::Mov,
    },
];

pub(super) const ACCUMULATOR_OFFSET_FORMS: [Form; 4] = [
    Form {
        opcode: 0xa0,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset {
            accumulator: RegisterRole::Destination,
        },
        width: WidthRule::Byte,
        semantic: Semantic::Mov,
    },
    Form {
        opcode: 0xa1,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset {
            accumulator: RegisterRole::Destination,
        },
        width: WidthRule::OperandSize,
        semantic: Semantic::Mov,
    },
    Form {
        opcode: 0xa2,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset {
            accumulator: RegisterRole::Source,
        },
        width: WidthRule::Byte,
        semantic: Semantic::Mov,
    },
    Form {
        opcode: 0xa3,
        mask: 0xff,
        encoding: Encoding::AccumulatorOffset {
            accumulator: RegisterRole::Source,
        },
        width: WidthRule::OperandSize,
        semantic: Semantic::Mov,
    },
];

/// Immediate payloads contain decoded bits; the instruction width gives them
/// their logical data type. Address components continue to use 32-bit values.
pub(super) enum Operand<V> {
    Immediate(V),
    Location(Location<V>),
}

pub(super) enum Location<V> {
    Register(RegisterCode),
    Memory(Address32<V>),
}

pub(super) struct Instruction<V> {
    pub(super) semantic: Semantic,
    pub(super) width: OperandWidth,
    pub(super) destination: Location<V>,
    pub(super) source: Operand<V>,
}

pub(super) struct DecodedInstruction<V, P> {
    pub(super) instruction: Instruction<V>,
    pub(super) eip: P,
    pub(super) next_eip: P,
}

impl<V> Instruction<V> {
    pub(super) fn uses_memory(&self) -> bool {
        matches!(self.destination, Location::Memory(_))
            || matches!(self.source, Operand::Location(Location::Memory(_)))
    }
}
