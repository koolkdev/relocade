//! Processor queries use full register widths in every execution profile.

use super::*;
use crate::register::Gpr32;

instruction_families! {
    CPUID {
        execute: cpuid;
        effects: [serializing];
        forms {
            0x0F 0xA2 => no_operands();
        }
    }
}

fn cpuid(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    let leaf = TypedLocation::<I32>::register(Gpr32::Eax).read(execution)?;
    let result = crate::processor::cpuid(&leaf);
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
