//! Processor queries and execution control.

use super::*;
use crate::register::Gpr32;

instruction_families! {
    HLT {
        execute: halt;
        effects: [control_transfer];
        flow: Fallthrough;
        availability: Privileged;
        forms { 0xF4 => no_operands(); }
    }
    CPUID {
        execute: cpuid;
        effects: [serializing];
        flow: Fallthrough;
        forms {
            0x0F 0xA2 => no_operands();
        }
    }
    RDTSC {
        execute: rdtsc;
        requires: [Tsc];
        forms {
            0x0F 0x31 => no_operands();
        }
    }
}

fn halt(
    execution: &mut ExecutionBuilder<'_, '_>,
    _condition: Option<Condition>,
    fallthrough: Val<I32>,
) -> Result<Val<I32>, BuildError> {
    execution.halt(&fallthrough)?;
    Ok(fallthrough)
}

fn cpuid(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let leaf = TypedLocation::<I32>::register(Gpr32::Eax).read(execution)?;
    let result = execution.cpuid(&leaf);
    for (register, value) in [
        (Gpr32::Eax, result.eax),
        (Gpr32::Ebx, result.ebx),
        (Gpr32::Ecx, result.ecx),
        (Gpr32::Edx, result.edx),
    ] {
        TypedLocation::<I32>::register(register).write(execution, value)?;
    }
    Ok(())
}

fn rdtsc(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let counter = execution.timestamp_counter()?;
    execution.write_register_pair::<I32>(Gpr32::Edx, Gpr32::Eax, counter)
}
