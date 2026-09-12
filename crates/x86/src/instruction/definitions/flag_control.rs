use super::*;
use crate::flags::Flag;
use wasm86_compiler::I1;

instruction_families! {
    CLC {
        execute: set(Flag::CF, false);
        forms {
            0xF8 => no_operands();
        }
    }
    STC {
        execute: set(Flag::CF, true);
        forms {
            0xF9 => no_operands();
        }
    }
    CMC {
        execute: complement_carry;
        forms {
            0xF5 => no_operands();
        }
    }
    CLD {
        execute: set(Flag::DF, false);
        forms {
            0xFC => no_operands();
        }
    }
    STD {
        execute: set(Flag::DF, true);
        forms {
            0xFD => no_operands();
        }
    }
}

fn set(
    execution: &mut ExecutionBuilder<'_, '_>,
    flag: Flag,
    value: impl Into<Val<I1>>,
) -> Result<(), BuildError> {
    execution.write_flag(flag, value)
}

fn complement_carry(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let carry = execution.read_flag(Flag::CF)?;
    execution.write_flag(Flag::CF, carry.xor(true))
}
