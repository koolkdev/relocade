use wasm86_compiler::{Val, I1, I8};

use crate::{address::Address32, register::RegisterCode};

#[derive(Clone, Copy)]
pub(super) enum Semantic {
    Mov,
}

#[derive(Clone, Copy)]
pub(super) enum Encoding {
    OpcodeRegisterImmediate,
    ModRm,
}

impl Encoding {
    pub(super) const fn operand_offset(self) -> u32 {
        match self {
            Self::OpcodeRegisterImmediate | Self::ModRm => 1,
        }
    }
}

/// Width of the instruction's data operands; effective addresses remain 32-bit.
#[derive(Clone, Copy)]
pub(super) enum OperandWidth {
    Byte,
    Dword,
}

impl OperandWidth {
    pub(super) const fn bytes(self) -> u32 {
        match self {
            Self::Byte => 1,
            Self::Dword => 4,
        }
    }
}

/// The register field is encoded in the opcode or ModRM.reg.
#[derive(Clone, Copy)]
enum Direction {
    RegisterDestination,
    RegisterSource,
}

pub(super) struct Form {
    opcode: u8,
    mask: u8,
    pub(super) encoding: Encoding,
    pub(super) width: OperandWidth,
    semantic: Semantic,
    direction: Direction,
}

impl Form {
    pub(super) const fn minimum_length(&self) -> u32 {
        self.encoding.operand_offset()
            + match self.encoding {
                Encoding::OpcodeRegisterImmediate => self.width.bytes(),
                Encoding::ModRm => 1,
            }
    }

    pub(super) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }

    pub(super) fn matches_value(&self, opcode: &Val<I8>) -> Val<I1> {
        opcode.and(u32::from(self.mask)).eq(u32::from(self.opcode))
    }

    pub(super) fn bind<V, P>(
        &self,
        register: RegisterCode,
        operand: Operand<V>,
        eip: P,
        next_eip: P,
    ) -> DecodedInstruction<V, P> {
        let (destination, source) = match self.direction {
            Direction::RegisterDestination => (Location::Register(register), operand),
            Direction::RegisterSource => {
                let Operand::Location(destination) = operand else {
                    unreachable!("a register-source form binds a ModRM location");
                };
                (destination, Operand::Location(Location::Register(register)))
            }
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

pub(super) const MOV_DWORD_IMMEDIATE: Form = Form {
    opcode: 0xb8,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    width: OperandWidth::Dword,
    semantic: Semantic::Mov,
    direction: Direction::RegisterDestination,
};

pub(super) const MOV_BYTE_IMMEDIATE: Form = Form {
    opcode: 0xb0,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate,
    width: OperandWidth::Byte,
    semantic: Semantic::Mov,
    direction: Direction::RegisterDestination,
};

pub(super) const IMMEDIATE_FORMS: [Form; 2] = [MOV_DWORD_IMMEDIATE, MOV_BYTE_IMMEDIATE];

pub(super) const MODRM_FORMS: [Form; 4] = [
    Form {
        opcode: 0x89,
        mask: 0xff,
        encoding: Encoding::ModRm,
        width: OperandWidth::Dword,
        semantic: Semantic::Mov,
        direction: Direction::RegisterSource,
    },
    Form {
        opcode: 0x8b,
        mask: 0xff,
        encoding: Encoding::ModRm,
        width: OperandWidth::Dword,
        semantic: Semantic::Mov,
        direction: Direction::RegisterDestination,
    },
    Form {
        opcode: 0x88,
        mask: 0xff,
        encoding: Encoding::ModRm,
        width: OperandWidth::Byte,
        semantic: Semantic::Mov,
        direction: Direction::RegisterSource,
    },
    Form {
        opcode: 0x8a,
        mask: 0xff,
        encoding: Encoding::ModRm,
        width: OperandWidth::Byte,
        semantic: Semantic::Mov,
        direction: Direction::RegisterDestination,
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
