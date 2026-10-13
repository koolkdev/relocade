//! Memory ordering and cache control have no SIMD register operands.

use super::*;

instruction_families! {
    LFENCE {
        execute: fence;
        forms { NP 0x0f 0xae @ 0xe8 + rm => no_operands(); }
    }
    MFENCE {
        execute: fence;
        forms { NP 0x0f 0xae @ 0xf0 + rm => no_operands(); }
    }
    SFENCE {
        execute: fence;
        forms { NP 0x0f 0xae @ 0xf8 + rm => no_operands(); }
    }
}

fn fence(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.memory_fence();
    Ok(())
}
