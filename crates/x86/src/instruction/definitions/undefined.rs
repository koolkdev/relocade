//! Instructions reserved for raising an invalid-opcode exception.

use super::*;
use crate::exception::Exception;

instruction_families! {
    UD2 {
        execute: raise_invalid_opcode;
        effects: [unconditional_fault];
        forms {
            0x0F 0x0B => no_operands();
        }
    }
}

fn raise_invalid_opcode(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.fault_if(true, Exception::InvalidOpcode)
}
