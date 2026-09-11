//! Instruction families keep their encodings and shared semantics together.

mod alu;
mod bit_scans;
mod bit_tests;
mod branches;
mod conditions;
mod divide;
mod exchanges;
mod moves;
mod multiply;
mod shifts;
mod stack;

use super::{
    forms::*,
    handlers::{
        binary_handlers, ternary_handlers, typed_operand, unary_handlers, Handler, IntegerHandlers,
        SizedHandlers,
    },
    Input, TypedLocation,
};
use crate::{alu::flags::Condition, execution::ExecutionBuilder};
use wasm86_compiler::{AtLeast, BuildError, Val, I16, I32, I8};

pub(crate) fn modrm_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    opcode_forms(map).filter(|form| form.encoding.has_modrm())
}

pub(crate) fn opcode_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    moves::forms()
        .chain(exchanges::FORMS.iter())
        .chain(alu::forms())
        .chain(multiply::FORMS.iter())
        .chain(divide::FORMS.iter())
        .chain(shifts::forms())
        .chain(bit_scans::FORMS.iter())
        .chain(bit_tests::forms())
        .chain(stack::FORMS.iter())
        .chain(conditions::FORMS.iter())
        .chain(branches::FORMS.iter())
        .filter(move |form| form.map == map)
}
