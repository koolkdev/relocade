//! MMX state control shares the x87 tag state.

use super::*;

instruction_families! {
    EMMS {
        execute: empty;
        requires: [Mmx];
        forms { NP 0x0f 0x77 => no_operands(); }
    }
}

fn empty(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.empty_mmx()
}
