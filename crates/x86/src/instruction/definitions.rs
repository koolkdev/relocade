//! Instruction families keep their encodings and shared semantics together.

mod adjust;
mod alu;
mod bit_counts;
mod bit_scans;
mod bit_tests;
mod bounds;
mod branches;
mod conditions;
mod divide;
mod exchanges;
mod extensions;
mod far_control;
mod flag_control;
mod flag_transfer;
mod moves;
mod multiply;
mod no_ops;
mod segments;
mod shifts;
mod stack;
mod strings;
mod undefined;

use super::{
    forms::*,
    handlers::{Handler, SizedHandlers},
    Input, TypedLocation,
};
use crate::execution::ExecutionBuilder;
use crate::flags::Condition;
use wasm86_compiler::{AtLeast, BuildError, Val, I16, I32, I8};

pub(crate) fn opcode_forms(map: OpcodeMap) -> impl Iterator<Item = &'static Form> + Clone {
    moves::forms()
        .chain(segments::forms())
        .chain(extensions::forms())
        .chain(exchanges::forms())
        .chain(no_ops::forms())
        .chain(alu::forms())
        .chain(adjust::forms())
        .chain(multiply::forms())
        .chain(divide::forms())
        .chain(shifts::forms())
        .chain(bit_counts::forms())
        .chain(bit_scans::forms())
        .chain(bit_tests::forms())
        .chain(bounds::forms())
        .chain(stack::forms())
        .chain(strings::forms())
        .chain(undefined::forms())
        .chain(conditions::forms())
        .chain(flag_control::forms())
        .chain(flag_transfer::forms())
        .chain(branches::forms())
        .chain(far_control::forms())
        .filter(move |form| form.map == map)
}
