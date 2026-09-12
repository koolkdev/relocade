//! Assigns decoded fields to the handler's arguments without reading guest state.

use super::{DecodedFields, LocationBinding, OperandBinding, OperandBindingShape, SizedForm};
use crate::{
    address::Address32,
    instruction::{
        handlers::{Handler, HandlerCall},
        DecodedInstruction, Instruction, Location, Operand,
    },
};

impl SizedForm {
    pub(crate) fn bind<V: Clone + From<u32>, P>(
        &self,
        fields: DecodedFields<V>,
        eip: P,
        fallthrough_eip: P,
    ) -> DecodedInstruction<V, P> {
        let call = match (self.handler, self.form.binding) {
            (Handler::Nullary(handler), OperandBindingShape::Nullary) => {
                HandlerCall::Nullary { handler }
            }
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
            (
                Handler::Ternary(handler),
                OperandBindingShape::Ternary {
                    destination,
                    first_source,
                    second_source,
                },
            ) => HandlerCall::Ternary {
                handler,
                destination: fields.bind_location(destination),
                first_source: fields.bind_operand(first_source),
                second_source: fields.bind_operand(second_source),
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

impl<V: Clone + From<u32>> DecodedFields<V> {
    fn bind_location(&self, binding: LocationBinding) -> Location<V> {
        match binding {
            LocationBinding::Register => {
                let (Self::OpcodeRegisterImmediate { register, .. }
                | Self::ModRm { register, .. }
                | Self::OpcodeRegister { register }) = self
                else {
                    unreachable!("the form selects a decoded register field")
                };
                Location::Register(register.clone().into())
            }
            LocationBinding::Rm => {
                let Self::ModRm { rm, .. } = self else {
                    unreachable!("the form selects a decoded r/m field")
                };
                rm.clone()
            }
            LocationBinding::FixedRegister(register) => Location::Register(register.into()),
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
            OperandBinding::Constant(bits) => Operand::Immediate(bits.into()),
            OperandBinding::Location(location) => self.bind_location(location).into(),
            OperandBinding::RmAddress => {
                let Location::Memory(address) = self.bind_location(LocationBinding::Rm) else {
                    unreachable!("address bindings require a memory addressing mode");
                };
                Operand::Address(address)
            }
            OperandBinding::Immediate => {
                let (Self::OpcodeRegisterImmediate { immediate, .. }
                | Self::Immediate { immediate }
                | Self::ModRm {
                    immediate: Some(immediate),
                    ..
                }) = self
                else {
                    unreachable!("the form selects a decoded immediate")
                };
                Operand::Immediate(immediate.clone())
            }
        }
    }
}
