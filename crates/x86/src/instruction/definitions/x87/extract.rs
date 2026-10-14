//! FXTRACT replaces ST(0) with the exponent and pushes the significand.

use super::*;
use crate::x87::Operand;

instruction_families! {
    FXTRACT {
        execute: extract();
        forms { 0xD9 @ 0xF4 => no_operands(); }
    }
}

fn extract(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.check_x87_exception()?;
    let top = execution.x87().read_stack(0)?;
    let empty = top.is_empty();
    let mut value = top.value;
    let full = execution.x87().push_full()?;
    let mut fault = empty.or(&full);
    let mut overflow = empty.eq(false).and(full);
    if execution.specialize_x87_operands(&mut fault, value.classification().normal())? {
        value = value.assume_normal();
        overflow = false.into();
    }
    let result = execution.calculate(|body| Operand::new(&value).extract(body))?;
    execution.record_x87_instruction()?;
    result::write_results(
        execution,
        [result.exponent, result.significand],
        &fault,
        result::Destination::PushPair {
            when: true.into(),
            overflow,
            rounding: result::PairRounding::Original,
        },
    )
}
