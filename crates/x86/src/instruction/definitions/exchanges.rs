use super::*;
use crate::alu::{AnyStatusSource, ArithmeticOp, StatusSource};
use crate::execution::PairValues;
use crate::register::{Gpr32, RegisterType};

instruction_families! {
    XCHG {
        execute: xchg;
        forms {
            0x86 => byte(rm, modrm_reg);
            0x87 => word_or_dword(rm, modrm_reg);
            0x90 +reg => word_or_dword(accumulator, opcode_reg);
        }
    }
    XADD {
        execute: xadd;
        forms {
            0x0F 0xC0 => byte(rm, modrm_reg);
            0x0F 0xC1 => word_or_dword(rm, modrm_reg);
        }
    }
    CMPXCHG {
        execute: cmpxchg;
        forms {
            0x0F 0xB0 => byte(rm, modrm_reg);
            0x0F 0xB1 => word_or_dword(rm, modrm_reg);
        }
    }
}

fn xchg<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    left: TypedLocation<T>,
    right: TypedLocation<T>,
) -> Result<(), BuildError> {
    left.update_pair(execution, right, |_, old| {
        Ok(PairValues {
            left: old.right,
            right: old.left,
        })
    })
}

fn xadd<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: TypedLocation<T>,
) -> Result<(), BuildError>
where
    StatusSource<T>: Into<AnyStatusSource>,
{
    destination.update_pair(execution, source, |execution, old| {
        let outcome = ArithmeticOp::Add.apply(old.left.clone(), old.right);
        let sum = outcome.result;
        execution.write_flags(outcome.flags)?;
        Ok(PairValues {
            left: sum,
            right: old.left,
        })
    })
}

fn cmpxchg<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    destination.update_pair(
        execution,
        TypedLocation::register(Gpr32::Eax),
        |execution, old| {
            let replacement = source.read(execution)?;
            let equal = old.right.eq(&old.left);
            execution.write_flags(
                ArithmeticOp::Subtract
                    .apply(old.right.clone(), old.left.clone())
                    .flags,
            )?;
            Ok(PairValues {
                left: equal.select(replacement, &old.left),
                right: equal.select(old.right, old.left),
            })
        },
    )
}
