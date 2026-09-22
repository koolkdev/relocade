//! Turns family rows into the physical layouts and bindings shared by both decoders.

mod adapters;
mod macros;

pub(in crate::instruction) use {adapters::*, macros::*};

use super::{
    Encoding, Form, HandlerBinding, ImmediateWidth, LocationBinding, ModRmSelector, OpcodeMap,
    OperandBinding, OperandEncoding,
};
use crate::flags::Condition;
use crate::instruction::handlers::{Handler, HandlerCall, SizedHandlers};
use crate::instruction::Group1Prefix;
use crate::register::NamedRegister;

#[derive(Clone, Copy)]
pub(in crate::instruction) enum OperandSpec {
    Rm,
    Memory,
    ModRmRegister,
    OpcodeRegister,
    FixedRegister(NamedRegister),
    Segment(crate::Segment),
    Offset,
    Immediate(ImmediateWidth),
    Constant(u32),
    Address,
}

impl OperandSpec {
    // Width alternatives may change logical types, but must decode and bind the
    // same physical fields. Named AX and EAX therefore share a binding here.
    const fn same_binding(self, other: Self) -> bool {
        match (self, other) {
            (Self::Rm, Self::Rm)
            | (Self::Memory, Self::Memory)
            | (Self::ModRmRegister, Self::ModRmRegister)
            | (Self::OpcodeRegister, Self::OpcodeRegister)
            | (Self::Offset, Self::Offset)
            | (Self::Address, Self::Address) => true,
            (Self::FixedRegister(left), Self::FixedRegister(right)) => left.same_location(right),
            (Self::Segment(left), Self::Segment(right)) => left as u8 == right as u8,
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
    SegmentLoad,
    /// Execution always raises a guest fault, so no successor belongs to the block.
    UnconditionalFault,
}

pub(in crate::instruction) struct Opcode {
    pub(in crate::instruction) map: OpcodeMap,
    pub(in crate::instruction) byte: u8,
    pub(in crate::instruction) group1_prefix: Option<Group1Prefix>,
    pub(in crate::instruction) register_range: bool,
    pub(in crate::instruction) modrm: Option<ModRmSelector>,
}

pub(in crate::instruction) struct Declaration<'a> {
    pub(in crate::instruction) opcode: Opcode,
    pub(in crate::instruction) operands: &'a [OperandSpec],
    pub(in crate::instruction) handlers: SizedHandlers<Handler>,
    pub(in crate::instruction) effects: &'a [Effect],
    pub(in crate::instruction) lockable: bool,
}

impl Declaration<'_> {
    pub(in crate::instruction) const fn form(self) -> Form {
        assert!(
            !matches!(self.opcode.group1_prefix, Some(Group1Prefix::F0)),
            "declare optional LOCK support with lockable"
        );
        if self.lockable {
            assert!(
                self.opcode.group1_prefix.is_none(),
                "LOCK eligibility requires an unprefixed form"
            );
            assert!(
                matches!(
                    self.operands.first(),
                    Some(OperandSpec::Rm | OperandSpec::Memory)
                ),
                "LOCK eligibility requires a ModRM memory destination"
            );
        }
        assert!(
            self.operands.len() <= 3,
            "instruction bodies take at most three operands"
        );
        // A selector requires the complete ModRM address encoding,
        // even when the instruction binds no operand to its handler.
        let mut modrm = self.opcode.modrm.is_some();
        let mut modrm_register = false;
        let mut opcode_register = false;
        let mut offset = false;
        let mut immediates = [None; 2];
        let mut immediate_count = 0;
        let mut memory_only = false;
        let mut bindings = [None; 3];
        let mut index = 0;
        while index < self.operands.len() {
            bindings[index] = Some(match self.operands[index] {
                OperandSpec::Rm | OperandSpec::Memory | OperandSpec::Address => {
                    modrm = true;
                    memory_only |= !matches!(self.operands[index], OperandSpec::Rm);
                    if matches!(self.operands[index], OperandSpec::Address) {
                        OperandBinding::RmAddress
                    } else {
                        OperandBinding::Location(LocationBinding::Rm)
                    }
                }
                OperandSpec::ModRmRegister => {
                    modrm = true;
                    modrm_register = true;
                    OperandBinding::Location(LocationBinding::Register)
                }
                OperandSpec::OpcodeRegister => {
                    opcode_register = true;
                    OperandBinding::Location(LocationBinding::Register)
                }
                OperandSpec::Offset => {
                    offset = true;
                    OperandBinding::Location(LocationBinding::AbsoluteOffset)
                }
                OperandSpec::Immediate(width) => {
                    assert!(
                        immediate_count < immediates.len(),
                        "a form has at most two immediate fields"
                    );
                    immediates[immediate_count] = Some(width);
                    let binding = OperandBinding::Immediate(immediate_count);
                    immediate_count += 1;
                    binding
                }
                OperandSpec::FixedRegister(register) => {
                    OperandBinding::Location(LocationBinding::FixedRegister(register))
                }
                OperandSpec::Constant(value) => OperandBinding::Constant(value),
                OperandSpec::Segment(segment) => OperandBinding::Segment(segment),
            });
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
            !(offset && (modrm || opcode_register || immediate_count != 0)),
            "moffs is a separate address layout"
        );
        if let Some(selector) = self.opcode.modrm {
            assert!(
                selector.mask & 0x38 == 0 || !modrm_register,
                "fixed ModRM.reg bits cannot also bind a register operand"
            );
        }
        let operands = if modrm {
            OperandEncoding::ModRm
        } else if opcode_register {
            OperandEncoding::OpcodeRegister
        } else if offset {
            OperandEncoding::AbsoluteOffset
        } else {
            OperandEncoding::None
        };
        let handlers = SizedHandlers {
            word: bind_handler(self.handlers.word, bindings),
            dword: bind_handler(self.handlers.dword, bindings),
        };
        let mut implicit_memory = false;
        let mut ends_block = false;
        index = 0;
        while index < self.effects.len() {
            match self.effects[index] {
                Effect::MemoryRead | Effect::MemoryWrite => implicit_memory = true,
                Effect::ControlTransfer | Effect::SegmentLoad | Effect::UnconditionalFault => {
                    ends_block = true
                }
            }
            index += 1;
        }
        Form {
            opcode: self.opcode.byte,
            mask: if opcode_register { 0xf8 } else { 0xff },
            map: self.opcode.map,
            group1_prefix: self.opcode.group1_prefix,
            lockable: self.lockable,
            modrm: if modrm {
                let selector = match self.opcode.modrm {
                    Some(selector) => selector,
                    None => ModRmSelector::any(),
                };
                Some(if memory_only {
                    selector.memory()
                } else {
                    selector
                })
            } else {
                None
            },
            encoding: Encoding {
                operands,
                immediates,
            },
            handlers,
            condition: None,
            implicit_memory,
            ends_block,
        }
    }
}

/// Declaration construction pairs each function with bindings of its exact arity.
const fn bind_handler(handler: Handler, bindings: [Option<OperandBinding>; 3]) -> HandlerBinding {
    match (handler, bindings) {
        (Handler::Nullary(handler), [None, None, None]) => HandlerCall::Nullary { handler },
        (Handler::Unary(handler), [Some(operand), None, None]) => {
            HandlerCall::Unary { handler, operand }
        }
        (Handler::Binary(handler), [Some(left), Some(right), None]) => HandlerCall::Binary {
            handler,
            left,
            right,
        },
        (
            Handler::Ternary(handler),
            [Some(OperandBinding::Location(destination)), Some(first_source), Some(second_source)],
        ) => HandlerCall::Ternary {
            handler,
            destination,
            first_source,
            second_source,
        },
        _ => panic!("the declaration must match the handler's arguments"),
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
