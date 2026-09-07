use wasm86_compiler::{Val, I1, I8};

use crate::{address::Address32, register::Register32};

#[derive(Clone, Copy)]
pub(super) enum Semantic {
    Mov32,
}

#[derive(Clone, Copy)]
pub(super) enum Encoding {
    OpcodeRegisterImmediate32,
    ModRm32,
}

impl Encoding {
    pub(super) const fn operand_offset(self) -> u32 {
        match self {
            Self::OpcodeRegisterImmediate32 | Self::ModRm32 => 1,
        }
    }

    pub(super) const fn minimum_length(self) -> u32 {
        match self {
            Self::OpcodeRegisterImmediate32 => self.operand_offset() + 4,
            Self::ModRm32 => self.operand_offset() + 1,
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
    semantic: Semantic,
    direction: Direction,
}

impl Form {
    pub(super) fn matches(&self, opcode: u8) -> bool {
        opcode & self.mask == self.opcode
    }

    pub(super) fn matches_value(&self, opcode: &Val<I8>) -> Val<I1> {
        opcode.and(u32::from(self.mask)).eq(u32::from(self.opcode))
    }

    pub(super) fn bind<V, P>(
        &self,
        register: Register32,
        operand: Operand32<V>,
        eip: P,
        next_eip: P,
    ) -> DecodedInstruction<V, P> {
        let (destination, source) = match self.direction {
            Direction::RegisterDestination => (Location32::Register(register), operand),
            Direction::RegisterSource => {
                let Operand32::Location(destination) = operand else {
                    unreachable!("a register-source form binds a ModRM location");
                };
                (
                    destination,
                    Operand32::Location(Location32::Register(register)),
                )
            }
        };
        DecodedInstruction {
            instruction: Instruction {
                semantic: self.semantic,
                destination,
                source,
            },
            eip,
            next_eip,
        }
    }
}

pub(super) const MOV_IMMEDIATE: Form = Form {
    opcode: 0xb8,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate32,
    semantic: Semantic::Mov32,
    direction: Direction::RegisterDestination,
};

pub(super) const MODRM_FORMS: [Form; 2] = [
    Form {
        opcode: 0x89,
        mask: 0xff,
        encoding: Encoding::ModRm32,
        semantic: Semantic::Mov32,
        direction: Direction::RegisterSource,
    },
    Form {
        opcode: 0x8b,
        mask: 0xff,
        encoding: Encoding::ModRm32,
        semantic: Semantic::Mov32,
        direction: Direction::RegisterDestination,
    },
];

pub(super) enum Operand32<V> {
    Immediate(V),
    Location(Location32<V>),
}

pub(super) enum Location32<V> {
    Register(Register32),
    Memory(Address32<V>),
}

pub(super) struct Instruction<V> {
    pub(super) semantic: Semantic,
    pub(super) destination: Location32<V>,
    pub(super) source: Operand32<V>,
}

pub(super) struct DecodedInstruction<V, P> {
    pub(super) instruction: Instruction<V>,
    pub(super) eip: P,
    pub(super) next_eip: P,
}

impl<V> Instruction<V> {
    pub(super) fn uses_memory(&self) -> bool {
        matches!(self.destination, Location32::Memory(_))
            || matches!(self.source, Operand32::Location(Location32::Memory(_)))
    }
}
