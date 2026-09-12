//! Instruction families keep their encodings and shared semantics together.

mod alu;
mod bit_scans;
mod bit_tests;
mod branches;
mod conditions;
mod divide;
mod exchanges;
mod extensions;
mod flag_control;
mod moves;
mod multiply;
mod shifts;
mod stack;

use super::{
    forms::*,
    handlers::{Handler, SizedHandlers},
    Input, TypedLocation,
};
use crate::execution::ExecutionBuilder;
use crate::flags::Condition;
use wasm86_compiler::{AtLeast, BuildError, Val, I16, I32, I8};

pub(crate) fn modrm_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    opcode_forms(map).filter(|form| form.encoding.has_modrm())
}

pub(crate) fn opcode_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    moves::forms()
        .chain(extensions::forms())
        .chain(exchanges::forms())
        .chain(alu::forms())
        .chain(multiply::forms())
        .chain(divide::forms())
        .chain(shifts::forms())
        .chain(bit_scans::forms())
        .chain(bit_tests::forms())
        .chain(stack::forms())
        .chain(conditions::forms())
        .chain(flag_control::forms())
        .chain(branches::forms())
        .filter(move |form| form.map == map)
}
