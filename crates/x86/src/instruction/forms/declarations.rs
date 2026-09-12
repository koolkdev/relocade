//! Turns family rows into the physical layouts and bindings shared by both decoders.

mod adapters;
mod macros;

pub(in crate::instruction) use {adapters::*, macros::*};

use super::{
    Encoding, Form, ImmediateWidth, LocationBinding, OpcodeMap, OperandBinding, OperandBindingShape,
};
use crate::flags::Condition;
use crate::instruction::handlers::{Handler, SizedHandlers};
use crate::register::NamedRegister;

#[derive(Clone, Copy)]
pub(in crate::instruction) enum OperandSpec {
    Rm,
    ModRmRegister,
    OpcodeRegister,
    FixedRegister(NamedRegister),
    Offset,
    Immediate(ImmediateWidth),
    Constant(u32),
    Address,
}

impl OperandSpec {
    const fn binding(self) -> OperandBinding {
        match self {
            Self::Immediate(_) => OperandBinding::Immediate,
            Self::Constant(value) => OperandBinding::Constant(value),
            Self::Address => OperandBinding::RmAddress,
            _ => OperandBinding::Location(self.location()),
        }
    }

    const fn location(self) -> LocationBinding {
        match self {
            Self::Rm => LocationBinding::Rm,
            Self::ModRmRegister | Self::OpcodeRegister => LocationBinding::Register,
            Self::FixedRegister(register) => LocationBinding::FixedRegister(register),
            Self::Offset => LocationBinding::AbsoluteOffset,
            _ => panic!("this handler argument requires a location"),
        }
    }

    // Width alternatives may change logical types, but must decode and bind the
    // same physical fields. Named AX and EAX therefore share a binding here.
    const fn same_binding(self, other: Self) -> bool {
        match (self, other) {
            (Self::Rm, Self::Rm)
            | (Self::ModRmRegister, Self::ModRmRegister)
            | (Self::OpcodeRegister, Self::OpcodeRegister)
            | (Self::Offset, Self::Offset)
            | (Self::Address, Self::Address) => true,
            (Self::FixedRegister(left), Self::FixedRegister(right)) => left.same_location(right),
            (Self::Immediate(left), Self::Immediate(right)) => left as u8 == right as u8,
            (Self::Constant(left), Self::Constant(right)) => left == right,
            _ => false,
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::instruction) enum Effect {
    MemoryRead,
    MemoryWrite,
    ControlTransfer,
}

pub(in crate::instruction) struct Opcode {
    pub(in crate::instruction) map: OpcodeMap,
    pub(in crate::instruction) byte: u8,
    pub(in crate::instruction) register_range: bool,
    pub(in crate::instruction) extension: Option<u8>,
}

pub(in crate::instruction) struct Declaration<'a> {
    pub(in crate::instruction) opcode: Opcode,
    pub(in crate::instruction) operands: &'a [OperandSpec],
    pub(in crate::instruction) handlers: SizedHandlers,
    pub(in crate::instruction) effects: &'a [Effect],
}

impl Declaration<'_> {
    pub(in crate::instruction) const fn form(self) -> Form {
        let mut modrm = false;
        let mut modrm_register = false;
        let mut opcode_register = false;
        let mut offset = false;
        let mut immediate = None;
        let mut index = 0;
        while index < self.operands.len() {
            match self.operands[index] {
                OperandSpec::Rm | OperandSpec::Address => modrm = true,
                OperandSpec::ModRmRegister => {
                    modrm = true;
                    modrm_register = true;
                }
                OperandSpec::OpcodeRegister => opcode_register = true,
                OperandSpec::Offset => offset = true,
                OperandSpec::Immediate(width) => {
                    assert!(
                        immediate.is_none(),
                        "a form has at most one immediate field"
                    );
                    immediate = Some(width);
                }
                OperandSpec::FixedRegister(_) | OperandSpec::Constant(_) => {}
            }
            index += 1;
        }
        assert!(
            opcode_register == self.opcode.register_range,
            "+reg requires an opcode_reg operand"
        );
        if opcode_register {
            assert!(
                self.opcode.byte & 7 == 0,
                "+reg starts on an eight-opcode boundary"
            );
        }
        assert!(
            !(modrm && opcode_register),
            "ModRM and opcode register fields cannot coexist"
        );
        assert!(
            !(offset && (modrm || opcode_register || immediate.is_some())),
            "moffs32 is a separate address layout"
        );
        if let Some(extension) = self.opcode.extension {
            assert!(
                extension < 8 && modrm && !modrm_register,
                "/n requires ModRM with no register operand"
            );
        }
        let encoding = if modrm {
            Encoding::ModRm { immediate }
        } else if opcode_register {
            match immediate {
                Some(immediate) => Encoding::OpcodeRegisterImmediate { immediate },
                None => Encoding::OpcodeRegister,
            }
        } else if offset {
            Encoding::AccumulatorOffset
        } else if let Some(immediate) = immediate {
            Encoding::Immediate { immediate }
        } else {
            Encoding::OpcodeOnly
        };
        let binding = match self.operands {
            [] => {
                assert!(matches!(
                    (self.handlers.word, self.handlers.dword),
                    (Handler::Nullary(_), Handler::Nullary(_))
                ));
                OperandBindingShape::Nullary
            }
            [operand] => {
                assert!(matches!(
                    (self.handlers.word, self.handlers.dword),
                    (Handler::Unary(_), Handler::Unary(_))
                ));
                OperandBindingShape::Unary(operand.binding())
            }
            [left, right] => {
                assert!(matches!(
                    (self.handlers.word, self.handlers.dword),
                    (Handler::Binary(_), Handler::Binary(_))
                ));
                OperandBindingShape::Binary {
                    left: left.location(),
                    right: right.binding(),
                }
            }
            [destination, first_source, second_source] => {
                assert!(matches!(
                    (self.handlers.word, self.handlers.dword),
                    (Handler::Ternary(_), Handler::Ternary(_))
                ));
                OperandBindingShape::Ternary {
                    destination: destination.location(),
                    first_source: first_source.binding(),
                    second_source: second_source.binding(),
                }
            }
            _ => panic!("instruction bodies take at most three operands"),
        };
        let mut implicit_memory = false;
        let mut ends_block = false;
        index = 0;
        while index < self.effects.len() {
            match self.effects[index] {
                Effect::MemoryRead | Effect::MemoryWrite => implicit_memory = true,
                Effect::ControlTransfer => ends_block = true,
            }
            index += 1;
        }
        Form {
            opcode: self.opcode.byte,
            mask: if opcode_register { 0xf8 } else { 0xff },
            map: self.opcode.map,
            extension: self.opcode.extension,
            encoding,
            handlers: self.handlers,
            binding,
            condition: None,
            implicit_memory,
            ends_block,
        }
    }
}

pub(in crate::instruction) const fn same_layout(left: &[OperandSpec], right: &[OperandSpec]) {
    assert!(
        left.len() == right.len(),
        "width alternatives have the same argument shape"
    );
    let mut index = 0;
    while index < left.len() {
        assert!(
            left[index].same_binding(right[index]),
            "width alternatives decode and bind the same fields"
        );
        index += 1;
    }
}

pub(in crate::instruction) const fn condition_forms(form: Form) -> [Form; 16] {
    assert!(
        form.opcode & 15 == 0 && form.mask == 0xff,
        "+cc starts on a sixteen-opcode boundary"
    );
    let mut forms = [form; 16];
    let mut code = 0;
    while code < 16 {
        forms[code].opcode += code as u8;
        forms[code].condition = Some(Condition::from_code(code as u8));
        code += 1;
    }
    forms
}
