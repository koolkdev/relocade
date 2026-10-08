use super::*;

instruction_families! {
    HLT {
        execute: halt;
        effects: [halt];
        availability: Privileged;
        forms { 0xF4 => no_operands(); }
    }
}

fn halt(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.halt();
    Ok(())
}
