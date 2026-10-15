//! Unprivileged queries of the current execution environment.

use super::*;
use crate::{address::MemoryAddress, register::RegisterType, DescriptorTable};

instruction_families! {
    SGDT {
        execute: store_descriptor_table::<_>(DescriptorTable::Global);
        forms { 0x0F 0x01 /0 => word_or_dword(mem); }
    }
    SIDT {
        execute: store_descriptor_table::<_>(DescriptorTable::Interrupt);
        forms { 0x0F 0x01 /1 => word_or_dword(mem); }
    }
    SMSW {
        execute: store_machine_status;
        forms { 0x0F 0x01 /4 => word_or_dword(rm); }
    }
}

fn store_machine_status<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let status = execution.profile().machine_status_word();
    // P4 leaves a dword register's upper half undefined; this model clears it.
    destination.write_word(execution, u32::from(status))
}

fn store_descriptor_table<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
    table: DescriptorTable,
) -> Result<(), BuildError> {
    execution.store_descriptor_table::<T>(destination, table)
}
