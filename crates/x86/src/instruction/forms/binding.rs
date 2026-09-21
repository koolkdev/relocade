//! Assigns decoded fields to the handler's arguments without reading guest state.

use super::{DecodedFields, LocationBinding, OperandBinding, ResolvedForm};
use crate::{
    address::EffectiveAddress,
    instruction::{handlers::HandlerCall, DecodedInstruction, Instruction, Location, Operand},
};

impl ResolvedForm {
    pub(crate) fn bind<V: Clone + From<u32>, P>(
        &self,
        fields: DecodedFields<V>,
        eip: P,
        fallthrough_eip: P,
    ) -> DecodedInstruction<V, P> {
        let call = match self.call {
            HandlerCall::Nullary { handler } => HandlerCall::Nullary { handler },
            HandlerCall::Binary {
                handler,
                left,
                right,
            } => HandlerCall::Binary {
                handler,
                left: self.bind_operand(&fields, left),
                right: self.bind_operand(&fields, right),
            },
            HandlerCall::Unary { handler, operand } => HandlerCall::Unary {
                handler,
                operand: self.bind_operand(&fields, operand),
            },
            HandlerCall::Ternary {
                handler,
                destination,
                first_source,
                second_source,
            } => HandlerCall::Ternary {
                handler,
                destination: self.bind_location(&fields, destination),
                first_source: self.bind_operand(&fields, first_source),
                second_source: self.bind_operand(&fields, second_source),
            },
        };
        DecodedInstruction {
            instruction: Instruction {
                call,
                address_size: self.address_size,
                condition: self.form.condition,
                implicit_memory: self.form.implicit_memory,
                ends_block: self.form.ends_block,
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
            LocationBinding::Register => Location::Register(
                fields
                    .register
                    .clone()
                    .expect("the form decodes a register")
                    .into(),
            ),
            LocationBinding::Rm => fields.rm.clone().expect("the form decodes r/m"),
            LocationBinding::FixedRegister(register) => Location::Register(register.into()),
            LocationBinding::AbsoluteOffset => Location::Memory(
                EffectiveAddress {
                    size: self.address_size,
                    base: None,
                    index: None,
                    displacement: fields
                        .absolute_offset
                        .clone()
                        .expect("the form decodes an absolute offset"),
                }
                .memory()
                .into(),
            ),
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
            OperandBinding::Segment(segment) => Operand::Segment(segment),
            OperandBinding::Constant(bits) => Operand::Immediate(bits.into()),
            OperandBinding::Location(location) => self.bind_location(fields, location).into(),
            OperandBinding::RmAddress => {
                let Location::Memory(address) = self.bind_location(fields, LocationBinding::Rm)
                else {
                    unreachable!("address bindings require a memory addressing mode");
                };
                Operand::Address(address.offset)
            }
            OperandBinding::Immediate(index) => Operand::Immediate(
                fields.immediates[index]
                    .clone()
                    .expect("the form decodes this immediate"),
            ),
        }
    }
}
