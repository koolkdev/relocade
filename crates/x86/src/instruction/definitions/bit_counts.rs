use super::*;
use crate::alu::{population_count, AnyStatusSource, StatusSource};
use crate::register::RegisterType;

instruction_families! {
    POPCNT {
        execute: popcnt;
        forms {
            F3 0x0F 0xB8 => word_or_dword(modrm_reg, rm);
        }
    }
}

fn popcnt<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let source = source.read(execution)?;
    let outcome = population_count(source);
    execution.write_flags(outcome.flags)?;
    destination.write(execution, outcome.result)
}
