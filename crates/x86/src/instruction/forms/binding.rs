//! Assigns decoded fields to the handler's arguments without reading guest state.

use super::{DecodedFields, LocationBinding, OperandBinding, OperandBindingShape, ResolvedForm};
use crate::{
    address::Address32,
    instruction::{
        handlers::{Handler, HandlerCall},
        DecodedInstruction, Instruction, Location, Operand,
    },
};

impl ResolvedForm {
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
                    left: self.bind_location(&fields, left),
                    right: self.bind_operand(&fields, right),
                }
            }
            (Handler::Unary(handler), OperandBindingShape::Unary(operand)) => HandlerCall::Unary {
                handler,
                operand: self.bind_operand(&fields, operand),
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
                destination: self.bind_location(&fields, destination),
                first_source: self.bind_operand(&fields, first_source),
                second_source: self.bind_operand(&fields, second_source),
            },
            _ => unreachable!("the form binds the handler's argument shape"),
        };
        DecodedInstruction {
            instruction: Instruction {
                call,
                condition: self.form.condition,
                implicit_memory: self.form.implicit_memory,
                ends_block: self.ends_block,
                segment_override: self.segment_override.clone(),
            },
            eip,
            fallthrough_eip,
        }
    }

    fn bind_location<V: Clone + From<u32>>(
        &self,
        fields: &DecodedFields<V>,
        binding: LocationBinding,
    ) -> Location<V> {
        let mut location = match binding {
            LocationBinding::Register => {
                let (DecodedFields::OpcodeRegisterImmediate { register, .. }
                | DecodedFields::ModRm { register, .. }
                | DecodedFields::OpcodeRegister { register }) = fields
                else {
                    unreachable!("the form selects a decoded register field")
                };
                Location::Register(register.clone().into())
            }
            LocationBinding::Rm => {
                let DecodedFields::ModRm { rm, .. } = fields else {
                    unreachable!("the form selects a decoded r/m field")
                };
                rm.clone()
            }
            LocationBinding::FixedRegister(register) => Location::Register(register.into()),
            LocationBinding::AbsoluteOffset => {
                let DecodedFields::AccumulatorOffset { offset } = fields else {
                    unreachable!("the form selects a decoded absolute offset")
                };
                Location::Memory(
                    Address32 {
                        base: None,
                        index: None,
                        displacement: offset.clone(),
                    }
                    .memory()
                    .into(),
                )
            }
        };
        if let Location::Memory(address) = &mut location {
            address.segment = self.segment_override.apply(&address.segment);
        }
        location
    }

    fn bind_operand<V: Clone + From<u32>>(
        &self,
        fields: &DecodedFields<V>,
        binding: OperandBinding,
    ) -> Operand<V> {
        match binding {
            OperandBinding::Constant(bits) => Operand::Immediate(bits.into()),
            OperandBinding::Location(location) => self.bind_location(fields, location).into(),
            OperandBinding::RmAddress => {
                let Location::Memory(address) = self.bind_location(fields, LocationBinding::Rm)
                else {
                    unreachable!("address bindings require a memory addressing mode");
                };
                Operand::Address(address.offset)
            }
            OperandBinding::Immediate => {
                let (DecodedFields::OpcodeRegisterImmediate { immediate, .. }
                | DecodedFields::Immediate { immediate }
                | DecodedFields::ModRm {
                    immediate: Some(immediate),
                    ..
                }) = fields
                else {
                    unreachable!("the form selects a decoded immediate")
                };
                Operand::Immediate(immediate.clone())
            }
        }
    }
}
