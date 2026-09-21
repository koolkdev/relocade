use super::*;

instruction_families! {
    NOP {
        execute: nop;
        forms {
            0x0F 0x1F /0 => no_operands();
        }
    }
}

fn nop(_execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    Ok(())
}
