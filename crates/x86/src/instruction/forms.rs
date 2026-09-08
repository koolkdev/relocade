mod catalog;

pub(crate) use catalog::*;

use super::{
    BinaryInstruction, BinaryOperation, DecodedInstruction, Instruction, Location, Operand,
    OperandSize, OperandWidth,
};
use crate::{address::Address32, flags::Condition, register::RegisterCode};
use wasm86_compiler::{Val, I1, I8};

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
    OpcodeRegisterImmediate,
    AccumulatorImmediate,
    RegisterRm {
        register: RegisterRole,
    },
    /// ModRM.reg selects an opcode extension; r/m names the left operand.
    RmImmediate {
        extension: u8,
        immediate: ImmediateWidth,
    },
    /// ModRM.reg is ignored; r/m names the only operand.
    Rm,
    /// The address field remains 32-bit regardless of the data width.
    AccumulatorOffset {
        accumulator: RegisterRole,
    },
}

impl Encoding {
    pub(crate) fn has_modrm(self) -> bool {
        matches!(
            self,
            Self::RegisterRm { .. } | Self::RmImmediate { .. } | Self::Rm
        )
    }
    pub(crate) fn matches_modrm(self, modrm: u8) -> bool {
        match self {
            Self::RmImmediate { extension, .. } => ((modrm >> 3) & 7) == extension,
            _ => true,
        }
    }
    pub(crate) fn extension_match(self, modrm: &Val<I8>) -> Option<Val<I1>> {
        match self {
            Self::RmImmediate { extension, .. } => {
                Some(modrm.and(0x38).eq(u32::from(extension) << 3))
            }
            _ => None,
        }
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

/// Position of the encoded register in the binary operand pair.
#[derive(Clone, Copy)]
pub(crate) enum RegisterRole {
    Left,
    Right,
}
impl RegisterRole {
    fn bind<V>(self, register: RegisterCode, other: Location<V>) -> (Location<V>, Operand<V>) {
        let register = Location::Register(register);
        match self {
            Self::Left => (register, Operand::Location(other)),
            Self::Right => (other, Operand::Location(register)),
        }
    }
}

/// Physical fields, before assignment to the operation's operand roles.
pub(crate) enum DecodedFields<V> {
    OpcodeRegisterImmediate {
        register: RegisterCode,
        immediate: V,
    },
    AccumulatorImmediate {
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
    Rm {
        rm: Location<V>,
    },
    AccumulatorOffset {
        offset: V,
    },
}

#[derive(Clone, Copy)]
enum Operation {
    Binary(BinaryOperation),
    SetCondition(Condition),
}

#[derive(Clone, Copy)]
pub(crate) struct Form {
    opcode: u8,
    mask: u8,
    pub(crate) map: OpcodeMap,
    pub(crate) encoding: Encoding,
    width: WidthRule,
    operation: Operation,
}
impl Form {
    pub(crate) const fn resolve(&self, size: OperandSize) -> ResolvedForm {
        ResolvedForm {
            encoding: self.encoding,
            width: self.width.resolve(size),
            map: self.map,
            operation: self.operation,
        }
    }
    pub(crate) fn same_opcode(&self, other: &Self) -> bool {
        self.map == other.map && self.opcode == other.opcode && self.mask == other.mask
    }
    /// The caller has already selected this form's opcode map.
    pub(crate) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }
    pub(crate) fn matches_value(&self, opcode: &Val<I8>) -> Val<I1> {
        opcode.and(u32::from(self.mask)).eq(u32::from(self.opcode))
    }
    pub(crate) fn matches_modrm_value(&self, opcode: &Val<I8>, modrm: &Val<I8>) -> Val<I1> {
        let opcode_match = self.matches_value(opcode);
        match self.encoding.extension_match(modrm) {
            Some(extension) => opcode_match.and(extension),
            None => opcode_match,
        }
    }
}

/// A form whose operand width is fixed before its fields are read.
#[derive(Clone, Copy)]
pub(crate) struct ResolvedForm {
    pub(crate) encoding: Encoding,
    pub(crate) width: OperandWidth,
    map: OpcodeMap,
    operation: Operation,
}
impl ResolvedForm {
    pub(crate) const fn immediate_width(&self) -> OperandWidth {
        match self.encoding {
            Encoding::RmImmediate {
                immediate: ImmediateWidth::SignedByte,
                ..
            } => OperandWidth::Byte,
            _ => self.width,
        }
    }
    pub(crate) fn sign_extends_immediate(&self) -> bool {
        matches!(
            self.encoding,
            Encoding::RmImmediate {
                immediate: ImmediateWidth::SignedByte,
                ..
            }
        )
    }
    /// Opcode bytes and shortest operand fields, excluding prefixes.
    pub(crate) const fn minimum_length(&self) -> u32 {
        self.map.bytes()
            + match self.encoding {
                Encoding::OpcodeRegisterImmediate | Encoding::AccumulatorImmediate => {
                    self.width.bytes()
                }
                Encoding::RegisterRm { .. } | Encoding::Rm => 1,
                Encoding::RmImmediate { .. } => 1 + self.immediate_width().bytes(),
                Encoding::AccumulatorOffset { .. } => 4,
            }
    }
    pub(crate) fn bind<V, P>(
        &self,
        fields: DecodedFields<V>,
        eip: P,
        next_eip: P,
    ) -> DecodedInstruction<V, P> {
        let instruction = match self.operation {
            Operation::SetCondition(condition) => {
                let DecodedFields::Rm { rm } = fields else {
                    unreachable!("SETcc has one r/m field")
                };
                Instruction::SetCondition {
                    condition,
                    destination: rm,
                }
            }
            Operation::Binary(operation) => {
                let (left, right) = self.bind_binary(fields);
                Instruction::Binary(BinaryInstruction {
                    operation,
                    width: self.width,
                    left,
                    right,
                })
            }
        };
        DecodedInstruction {
            instruction,
            eip,
            next_eip,
        }
    }
    fn bind_binary<V>(&self, fields: DecodedFields<V>) -> (Location<V>, Operand<V>) {
        match (self.encoding, fields) {
            (
                Encoding::OpcodeRegisterImmediate,
                DecodedFields::OpcodeRegisterImmediate {
                    register,
                    immediate,
                },
            ) => (Location::Register(register), Operand::Immediate(immediate)),
            (Encoding::AccumulatorImmediate, DecodedFields::AccumulatorImmediate { immediate }) => {
                (
                    Location::Register(RegisterCode::from_code(0)),
                    Operand::Immediate(immediate),
                )
            }
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
        }
    }
}
