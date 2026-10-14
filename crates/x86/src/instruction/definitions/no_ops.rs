use super::*;

instruction_families! {
    NOP {
        execute: nop;
        forms {
            0x0F 0x1F /0 => no_operands();
        }
    }
    PAUSE {
        execute: nop;
        forms {
            F3 0x90 => no_operands();
        }
    }
}

// PAUSE permits a zero-length delay, so it shares NOP's empty semantic body.
fn nop(_execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    Ok(())
}
