use super::*;
use crate::address::MemoryAddress;
use crate::alu::{
    compare_exchange, compare_exchange8b, AnyStatusSource, ArithmeticOp, StatusSource,
};
use crate::execution::PairValues;
use crate::register::{Gpr32, RegisterType};
use wasm86_compiler::I64;

instruction_families! {
    XCHG {
        execute: xchg;
        forms {
            0x86 => byte(rm, modrm_reg) lockable;
            0x87 => word_or_dword(rm, modrm_reg) lockable;
            0x90 +reg => word_or_dword(accumulator, opcode_reg);
        }
    }
    XADD {
        execute: xadd;
        forms {
            0x0F 0xC0 => byte(rm, modrm_reg) lockable;
            0x0F 0xC1 => word_or_dword(rm, modrm_reg) lockable;
        }
    }
    CMPXCHG {
        execute: cmpxchg;
        forms {
            0x0F 0xB0 => byte(rm, modrm_reg) lockable;
            0x0F 0xB1 => word_or_dword(rm, modrm_reg) lockable;
        }
    }
    CMPXCHG8B {
        execute: cmpxchg8b;
        forms {
            0x0F 0xC7 /1 => qword(mem) lockable;
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
            let outcome = compare_exchange(old.left, old.right, replacement);
            execution.write_flags(outcome.flags)?;
            Ok(PairValues {
                left: outcome.destination,
                right: outcome.accumulator,
            })
        },
    )
}

fn cmpxchg8b(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    execution.update_memory::<I64>(destination, |execution, previous| {
        let accumulator = execution.read_register_pair::<I32>(Gpr32::Edx, Gpr32::Eax)?;
        let replacement = execution.read_register_pair::<I32>(Gpr32::Ecx, Gpr32::Ebx)?;
        let outcome = compare_exchange8b(previous, accumulator, replacement);
        execution.write_flags(outcome.flags)?;
        execution.write_register_pair::<I32>(Gpr32::Edx, Gpr32::Eax, outcome.accumulator)?;
        Ok(outcome.destination)
    })
}
