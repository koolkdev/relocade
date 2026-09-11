use super::*;
use crate::register::RegisterType;

instruction_families! {
    SETcc {
        execute: setcc;
        forms {
            0x0F 0x90 +cc => byte(rm);
        }
    }
    CMOVcc {
        execute: cmov;
        forms {
            0x0F 0x40 +cc => word_or_dword(modrm_reg, rm);
        }
    }
}

fn setcc(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I8>,
    condition: Condition,
) -> Result<(), BuildError> {
    let value = execution.condition(condition)?;
    destination.write(execution, value.unsigned().extend::<I8>())
}

fn cmov<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
    condition: Condition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    // The source access can fault even when the condition is false.
    let value = source.read(execution)?;
    let predicate = execution.condition(condition)?;
    destination.update(execution, |_, previous| {
        Ok(predicate.select(value, previous))
    })
}
