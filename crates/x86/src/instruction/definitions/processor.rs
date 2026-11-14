//! Processor queries and execution control.

use super::*;
use crate::{
    address::MemoryAddress,
    memory::Intent,
    register::{Gpr32, RegisterType},
};

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
    SGDT {
        execute: store_global_descriptor_table::<_>;
        forms { 0x0F 0x01 /0 => word_or_dword(mem); }
    }
    SMSW {
        execute: store_machine_status_word;
        forms { 0x0F 0x01 /4 => word_or_dword(rm); }
    }
}

fn store_machine_status_word<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let status = execution.machine_status_word()?;
    // P4 leaves the upper word of a dword register destination undefined; choose zero.
    destination.write_word(execution, status)
}

fn store_global_descriptor_table<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    let table = execution.global_descriptor_table()?;
    // P4's 16-bit operand size stores a 24-bit base and zero in the sixth byte.
    let base = if T::BYTES == 2 {
        table.base.and(0x00ff_ffffu32)
    } else {
        table.base
    };
    let target = execution.memory_operand(destination, 6, Intent::Write, &[])?;
    target.write::<I16>(execution, 0, table.limit)?;
    target.write::<I32>(execution, 2, base)
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
    let counter = execution.instruction_count()?;
    execution.write_register_pair::<I32>(Gpr32::Edx, Gpr32::Eax, counter)
}
