//! Processor queries use full register widths in every execution profile.

use super::*;

instruction_families! {
    CPUID {
        execute: cpuid;
        effects: [serializing];
        forms {
            0x0F 0xA2 => no_operands();
        }
    }
    RDTSC {
        execute: rdtsc;
        forms {
            0x0F 0x31 => no_operands();
        }
    }
}

fn cpuid(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.cpuid()
}

fn rdtsc(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    execution.rdtsc()
}
