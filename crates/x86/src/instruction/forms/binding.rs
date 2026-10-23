//! Assigns decoded fields to the handler's arguments without reading guest state.

use super::{DecodedFields, LocationBinding, OperandBinding, RegisterClass, ResolvedForm};
use crate::{
    address::EffectiveAddress,
    instruction::{
        handlers::HandlerCall, DecodedInstruction, Instruction, Location, Operand, X87Opcode,
    },
};

impl RegisterClass {
    fn location<V>(self, code: crate::register::RegisterCode) -> Location<V> {
        match self {
            Self::Gpr => Location::Register(code.into()),
            Self::Mmx => Location::Mmx(code),
            Self::Xmm => Location::Xmm(code),
        }
    }
}

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
                ends_block: self.form.ends_block,
                requirements: self.form.requirements,
                segment_override: self.segment_override.clone(),
                locked: self.locked,
                x87_opcode: (self.form.map == super::OpcodeMap::Primary
                    && (0xd8..=0xdf).contains(&self.form.opcode))
                .then(|| X87Opcode {
                    primary: self.form.opcode,
                    modrm: fields
                        .modrm
                        .clone()
                        .expect("x87 encodings have a ModRM byte"),
                }),
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
            LocationBinding::Register(class) => class.location(
                fields
                    .register
                    .clone()
                    .expect("the form decodes a register"),
            ),
            LocationBinding::Rm(class) => match fields.rm.clone().expect("the form decodes r/m") {
                Location::Register(crate::register::RegisterOperand::Encoded(code)) => {
                    class.location(code)
                }
                memory @ Location::Memory(_) => memory,
                _ => unreachable!("decoded r/m registers retain their encoded field"),
            },
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
            OperandBinding::X87StackIndex => {
                Operand::X87StackIndex(fields.rm_index.clone().expect("the form decodes ModRM.r/m"))
            }
            OperandBinding::Segment(segment) => Operand::Segment(segment),
            OperandBinding::Constant(bits) => Operand::Immediate(bits.into()),
            OperandBinding::Location(location) => self.bind_location(fields, location).into(),
            OperandBinding::RmAddress => {
                let Location::Memory(address) =
                    self.bind_location(fields, LocationBinding::Rm(RegisterClass::Gpr))
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
