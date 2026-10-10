//! x87 conditional moves consume integer conditions without changing EFLAGS.

use super::*;
use crate::instruction::X87StackIndex;

instruction_families! {
    FCMOVB {
        execute: move_register(Condition::B);
        forms { 0xDA @ 0xC0 + rm => operands(st); }
    }
    FCMOVE {
        execute: move_register(Condition::E);
        forms { 0xDA @ 0xC8 + rm => operands(st); }
    }
    FCMOVBE {
        execute: move_register(Condition::BE);
        forms { 0xDA @ 0xD0 + rm => operands(st); }
    }
    FCMOVU {
        execute: move_register(Condition::P);
        forms { 0xDA @ 0xD8 + rm => operands(st); }
    }
    FCMOVNB {
        execute: move_register(Condition::AE);
        forms { 0xDB @ 0xC0 + rm => operands(st); }
    }
    FCMOVNE {
        execute: move_register(Condition::NE);
        forms { 0xDB @ 0xC8 + rm => operands(st); }
    }
    FCMOVNBE {
        execute: move_register(Condition::A);
        forms { 0xDB @ 0xD0 + rm => operands(st); }
    }
    FCMOVNU {
        execute: move_register(Condition::NP);
        forms { 0xDB @ 0xD8 + rm => operands(st); }
    }
}

fn move_register(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: X87StackIndex,
    condition: Condition,
) -> Result<(), BuildError> {
    execution.conditional_move_x87(source.offset(), condition)
}
