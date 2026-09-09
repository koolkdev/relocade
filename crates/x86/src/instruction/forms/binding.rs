//! Resolves each operand's declared source and constructs semantic instructions.

use super::{DecodedFields, LocationBinding, OperandBinding, Operation, SizedForm};
use crate::{
    address::Address32,
    instruction::{
        BinaryInstruction, DecodedInstruction, Instruction, Location, Operand, UnaryInstruction,
    },
    register::RegisterCode,
};

impl SizedForm {
    pub(crate) fn bind<V: Clone, P>(
        &self,
        fields: DecodedFields<V>,
        eip: P,
        next_eip: P,
    ) -> DecodedInstruction<V, P> {
        let instruction = match self.operation {
            Operation::SetCondition(condition) => {
                let DecodedFields::Location(destination) = fields else {
                    unreachable!("SETcc has one decoded location")
                };
                Instruction::SetCondition {
                    condition,
                    destination,
                }
            }
            Operation::Unary(operation) => {
                let DecodedFields::Location(destination) = fields else {
                    unreachable!("unary instructions have one decoded location")
                };
                Instruction::Unary(UnaryInstruction {
                    operation,
                    width: self.width,
                    destination,
                })
            }
            Operation::Push(source) => Instruction::Push {
                width: self.width,
                source: fields.bind_operand(source),
            },
            Operation::Pop(destination) => Instruction::Pop {
                width: self.width,
                destination: fields.bind_location(destination),
            },
            Operation::Binary {
                operation,
                left,
                right,
            } => Instruction::Binary(BinaryInstruction {
                operation,
                width: self.width,
                left: fields.bind_location(left),
                right: fields.bind_operand(right),
            }),
        };
        DecodedInstruction {
            instruction,
            eip,
            next_eip,
        }
    }
}

impl<V: Clone> DecodedFields<V> {
    fn bind_location(&self, binding: LocationBinding) -> Location<V> {
        match binding {
            LocationBinding::Register => {
                let (Self::OpcodeRegisterImmediate { register, .. }
                | Self::RegisterRm { register, .. }
                | Self::Location(Location::Register(register))) = self
                else {
                    unreachable!("the form selects a decoded register field")
                };
                Location::Register(register.clone())
            }
            LocationBinding::Rm => {
                let (Self::RegisterRm { rm, .. }
                | Self::RmImmediate { rm, .. }
                | Self::Location(rm)) = self
                else {
                    unreachable!("the form selects a decoded r/m field")
                };
                rm.clone()
            }
            LocationBinding::Accumulator => Location::Register(RegisterCode::from_code(0)),
            LocationBinding::AbsoluteOffset => {
                let Self::AccumulatorOffset { offset } = self else {
                    unreachable!("the form selects a decoded absolute offset")
                };
                Location::Memory(Address32 {
                    base: None,
                    index: None,
                    displacement: offset.clone(),
                })
            }
        }
    }

    fn bind_operand(&self, binding: OperandBinding) -> Operand<V> {
        match binding {
            OperandBinding::Location(location) => self.bind_location(location).into(),
            OperandBinding::Immediate => {
                let (Self::OpcodeRegisterImmediate { immediate, .. }
                | Self::Immediate { immediate }
                | Self::RmImmediate { immediate, .. }) = self
                else {
                    unreachable!("the form selects a decoded immediate")
                };
                Operand::Immediate(immediate.clone())
            }
        }
    }
}
