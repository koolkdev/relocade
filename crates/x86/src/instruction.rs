use crate::register::Register32;

#[derive(Clone, Copy)]
pub(super) enum Semantic {
    Mov32,
}

#[derive(Clone, Copy)]
pub(super) enum Encoding {
    OpcodeRegisterImmediate32,
    ModRmRegister32,
}

impl Encoding {
    pub(super) const fn operand_offset(self) -> u32 {
        match self {
            Self::OpcodeRegisterImmediate32 | Self::ModRmRegister32 => 1,
        }
    }

    pub(super) const fn length(self) -> u32 {
        match self {
            Self::OpcodeRegisterImmediate32 => self.operand_offset() + 4,
            Self::ModRmRegister32 => self.operand_offset() + 1,
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
    pub(super) opcode: u8,
    pub(super) mask: u8,
    pub(super) encoding: Encoding,
    semantic: Semantic,
    direction: Direction,
}

impl Form {
    pub(super) fn bind<V, P>(
        &self,
        register: Register32,
        operand: Operand32<V>,
        next_eip: P,
    ) -> DecodedInstruction<V, P> {
        let (destination, source) = match self.direction {
            Direction::RegisterDestination => (register, operand),
            Direction::RegisterSource => {
                let Operand32::Register(destination) = operand else {
                    unreachable!("a register-source form has a register destination");
                };
                (destination, Operand32::Register(register))
            }
        };
        DecodedInstruction {
            instruction: Instruction {
                semantic: self.semantic,
                destination,
                source,
            },
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

pub(super) const REGISTER_FORMS: [Form; 2] = [
    Form {
        opcode: 0x89,
        mask: 0xff,
        encoding: Encoding::ModRmRegister32,
        semantic: Semantic::Mov32,
        direction: Direction::RegisterSource,
    },
    Form {
        opcode: 0x8b,
        mask: 0xff,
        encoding: Encoding::ModRmRegister32,
        semantic: Semantic::Mov32,
        direction: Direction::RegisterDestination,
    },
];

pub(super) enum Operand32<V> {
    Immediate(V),
    Register(Register32),
}

pub(super) struct Instruction<V> {
    pub(super) semantic: Semantic,
    pub(super) destination: Register32,
    pub(super) source: Operand32<V>,
}

pub(super) struct DecodedInstruction<V, P> {
    pub(super) instruction: Instruction<V>,
    pub(super) next_eip: P,
}
