use crate::register::Register32;

#[derive(Clone, Copy)]
pub(super) enum Semantic {
    Mov32,
}

#[derive(Clone, Copy)]
pub(super) enum Encoding {
    OpcodeRegisterImmediate32,
}

impl Encoding {
    pub(super) const fn immediate_offset(self) -> u32 {
        match self {
            Self::OpcodeRegisterImmediate32 => 1,
        }
    }

    pub(super) const fn length(self) -> u32 {
        match self {
            Self::OpcodeRegisterImmediate32 => self.immediate_offset() + 4,
        }
    }
}

pub(super) struct Form {
    pub(super) opcode: u8,
    pub(super) mask: u8,
    pub(super) encoding: Encoding,
    pub(super) semantic: Semantic,
}

impl Form {
    pub(super) fn bind<V, P>(
        &self,
        opcode_register: Register32,
        immediate: V,
        next_eip: P,
    ) -> DecodedInstruction<V, P> {
        let instruction = match self.encoding {
            Encoding::OpcodeRegisterImmediate32 => Instruction {
                semantic: self.semantic,
                destination: opcode_register,
                source: immediate,
            },
        };
        DecodedInstruction {
            instruction,
            next_eip,
        }
    }
}

pub(super) const MOV_IMMEDIATE: Form = Form {
    opcode: 0xb8,
    mask: 0xf8,
    encoding: Encoding::OpcodeRegisterImmediate32,
    semantic: Semantic::Mov32,
};

pub(super) struct Instruction<V> {
    pub(super) semantic: Semantic,
    pub(super) destination: Register32,
    pub(super) source: V,
}

pub(super) struct DecodedInstruction<V, P> {
    pub(super) instruction: Instruction<V>,
    pub(super) next_eip: P,
}
