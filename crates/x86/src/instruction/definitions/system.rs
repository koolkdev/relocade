//! Unprivileged queries of the current execution environment.

use super::*;
use crate::register::RegisterType;

instruction_families! {
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
