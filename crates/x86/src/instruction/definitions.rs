//! Instruction families keep their encodings and shared semantics together.

mod alu;
mod branches;
mod conditions;
mod exchanges;
mod moves;
mod shifts;
mod stack;

use super::{
    forms::*,
    handlers::{
        binary_handlers, typed_operand, unary_handlers, Handler, IntegerHandlers, SizedHandlers,
    },
    Input, TypedLocation,
};
use crate::{execution::ExecutionBuilder, flags::Condition};
use wasm86_compiler::{AtLeast, BuildError, Val, I16, I32, I8};

pub(crate) fn modrm_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    opcode_forms(map).filter(|form| form.encoding.has_modrm())
}

pub(crate) fn opcode_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    moves::forms()
        .chain(exchanges::FORMS.iter())
        .chain(alu::forms())
        .chain(shifts::forms())
        .chain(stack::FORMS.iter())
        .chain(conditions::FORMS.iter())
        .chain(branches::FORMS.iter())
        .filter(move |form| form.map == map)
}
