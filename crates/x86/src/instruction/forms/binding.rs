//! Assigns decoded fields to the handler's arguments without reading guest state.

use super::{DecodedFields, LocationBinding, OperandBinding, OperandBindingShape, SizedForm};
use crate::{
    address::Address32,
    instruction::{
        handlers::{Handler, HandlerCall},
        DecodedInstruction, Instruction, Location, Operand,
    },
    register::RegisterCode,
};

impl SizedForm {
    pub(crate) fn bind<V: Clone, P>(
        &self,
        fields: DecodedFields<V>,
        eip: P,
        fallthrough_eip: P,
    ) -> DecodedInstruction<V, P> {
        let call = match (self.handler, self.form.binding) {
            (Handler::Binary(handler), OperandBindingShape::Binary { left, right }) => {
                HandlerCall::Binary {
                    handler,
                    left: fields.bind_location(left),
                    right: fields.bind_operand(right),
                }
            }
            (Handler::Unary(handler), OperandBindingShape::Unary(operand)) => HandlerCall::Unary {
                handler,
                operand: fields.bind_operand(operand),
            },
            _ => unreachable!("the form binds the handler's argument shape"),
        };
        DecodedInstruction {
            instruction: Instruction {
                call,
                condition: self.form.condition,
                implicit_memory: self.form.implicit_memory,
                ends_block: self.form.ends_block,
            },
            eip,
            fallthrough_eip,
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
