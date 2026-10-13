//! FXAM reports ST(0)'s class and sign without raising operand exceptions.

use super::*;
use crate::state::x87::ConditionCode;

instruction_families! {
    FXAM {
        execute: examine;
        forms { 0xD9 @ 0xE5 => no_operands(); }
    }
}

fn examine(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    execution.record_x87_instruction()?;
    let top = execution.x87().read_stack(0)?;
    let empty = top.is_empty();
    let class = top.value.classification();
    // Empty overrides the payload's class. Unsupported extended encodings
    // override predicates such as NaN that may also match their raw bits.
    let supported = empty.or(class.unsupported()).eq(false);
    let infinity = class.infinity();
    let denormal = class.denormal();
    let mut x87 = execution.x87();
    x87.set_condition_code(
        ConditionCode::C0,
        empty.or(supported.and(class.nan().or(&infinity))),
    )?;
    // The sign remains observable even when ST(0) is empty.
    x87.set_condition_code(ConditionCode::C1, top.value.negative())?;
    x87.set_condition_code(
        ConditionCode::C2,
        supported.and(class.normal().or(&denormal).or(infinity)),
    )?;
    x87.set_condition_code(
        ConditionCode::C3,
        empty.or(supported.and(class.zero().or(denormal))),
    )
}
