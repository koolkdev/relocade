//! MXCSR transfers validate and publish state through the execution owner.

use super::*;
use crate::{address::MemoryAddress, memory::Intent};

instruction_families! {
    LDMXCSR {
        execute: load_mxcsr;
        forms { NP 0x0f 0xae / 2 => operands(mem); }
    }
    STMXCSR {
        execute: store_mxcsr;
        forms { NP 0x0f 0xae / 3 => operands(mem); }
    }
}

fn load_mxcsr(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    let value = execution
        .memory_operand(source, 4, Intent::Read, &[])?
        .read(execution, 0)?;
    execution.load_mxcsr(value)
}

fn store_mxcsr(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    let value = execution.read_mxcsr()?.bits();
    execution
        .memory_operand(destination, 4, Intent::Write, &[])?
        .write(execution, 0, value)
}
