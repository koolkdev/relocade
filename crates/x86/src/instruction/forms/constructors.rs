//! Shared physical layouts and operand roles used by instruction definitions.

use super::{
    Encoding, Form, ImmediateWidth, LocationBinding, OpcodeMap, OperandBinding, OperandBindingShape,
};
use crate::instruction::handlers::{Handler, SizedHandlers};

/// The binary argument selected by ModRM.reg, independent of reads and writes.
#[derive(Clone, Copy)]
pub(in crate::instruction) enum RegisterSide {
    Left,
    Right,
}

pub(in crate::instruction) const fn primary_form(
    opcode: u8,
    encoding: Encoding,
    handlers: SizedHandlers<Handler>,
    binding: OperandBindingShape,
) -> Form {
    Form {
        opcode,
        mask: 0xff,
        map: OpcodeMap::Primary,
        encoding,
        extension: None,
        handlers,
        binding,
        condition: None,
        implicit_memory: false,
        ends_block: false,
    }
}

pub(in crate::instruction) const fn opcode_register(
    opcode: u8,
    handlers: SizedHandlers<Handler>,
) -> Form {
    let mut form = primary_form(
        opcode,
        Encoding::OpcodeRegister,
        handlers,
        OperandBindingShape::Unary(OperandBinding::Location(LocationBinding::Register)),
    );
    form.mask = 0xf8;
    form
}

pub(in crate::instruction) const fn rm(
    opcode: u8,
    extension: u8,
    handlers: SizedHandlers<Handler>,
) -> Form {
    let mut form = primary_form(
        opcode,
        Encoding::ModRm { immediate: None },
        handlers,
        OperandBindingShape::Unary(OperandBinding::Location(LocationBinding::Rm)),
    );
    form.extension = Some(extension);
    form
}

pub(in crate::instruction) const fn register_rm(
    map: OpcodeMap,
    opcode: u8,
    handlers: SizedHandlers<Handler>,
    register_side: RegisterSide,
) -> Form {
    let (left, right) = match register_side {
        RegisterSide::Left => (LocationBinding::Register, LocationBinding::Rm),
        RegisterSide::Right => (LocationBinding::Rm, LocationBinding::Register),
    };
    let mut form = primary_form(
        opcode,
        Encoding::ModRm { immediate: None },
        handlers,
        OperandBindingShape::Binary {
            left,
            right: OperandBinding::Location(right),
        },
    );
    form.map = map;
    form
}

pub(in crate::instruction) const fn accumulator_immediate(
    opcode: u8,
    immediate: ImmediateWidth,
    handlers: SizedHandlers<Handler>,
) -> Form {
    primary_form(
        opcode,
        Encoding::Immediate { immediate },
        handlers,
        OperandBindingShape::Binary {
            left: LocationBinding::Accumulator,
            right: OperandBinding::Immediate,
        },
    )
}

pub(in crate::instruction) const fn rm_immediate(
    map: OpcodeMap,
    opcode: u8,
    extension: u8,
    immediate: ImmediateWidth,
    handlers: SizedHandlers<Handler>,
) -> Form {
    let mut form = primary_form(
        opcode,
        Encoding::ModRm {
            immediate: Some(immediate),
        },
        handlers,
        OperandBindingShape::Binary {
            left: LocationBinding::Rm,
            right: OperandBinding::Immediate,
        },
    );
    form.extension = Some(extension);
    form.map = map;
    form
}
