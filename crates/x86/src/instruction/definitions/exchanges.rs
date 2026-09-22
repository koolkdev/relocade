use super::*;
use crate::address::MemoryAddress;
use crate::alu::{AnyStatusSource, ArithmeticOp, OperandUpdate, StatusSource};
use crate::flags::{Flag, FlagChange};
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
    let left = left.prepare_write(execution, &[])?;
    let right = right.prepare_write(execution, &[])?;
    let replacement = right.read(execution)?;
    left.modify(
        execution,
        OperandUpdate::Exchange(replacement),
        true,
        |execution, previous| right.write(execution, previous),
    )
}

fn xadd<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    source: TypedLocation<T>,
) -> Result<(), BuildError>
where
    StatusSource<T>: Into<AnyStatusSource>,
{
    let destination = destination.prepare_write(execution, &[])?;
    let source = source.prepare_write(execution, &[])?;
    let addend = source.read(execution)?;
    let locked = execution.is_locked();
    destination.modify(
        execution,
        OperandUpdate::Add(addend.clone()),
        locked,
        |execution, previous| {
            execution.write_flags(ArithmeticOp::Add.apply(previous.clone(), addend).flags)?;
            source.write(execution, previous)
        },
    )
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
    let destination = destination.prepare_write(execution, &[])?;
    let accumulator = TypedLocation::<T>::register(Gpr32::Eax).prepare_write(execution, &[])?;
    let expected = accumulator.read(execution)?;
    let replacement = source.read(execution)?;
    let update = OperandUpdate::CompareExchange {
        expected: expected.clone(),
        replacement,
    };
    let locked = execution.is_locked();
    destination.modify(execution, update, locked, |execution, previous| {
        execution.write_flags(
            ArithmeticOp::Subtract
                .apply(expected, previous.clone())
                .flags,
        )?;
        accumulator.write(execution, previous)
    })
}

fn cmpxchg8b(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: MemoryAddress<Val<I32>>,
) -> Result<(), BuildError> {
    let accumulator = execution.read_register_pair::<I32>(Gpr32::Edx, Gpr32::Eax)?;
    let replacement = execution.read_register_pair::<I32>(Gpr32::Ecx, Gpr32::Ebx)?;
    let previous = execution.modify_memory::<I64>(
        destination,
        OperandUpdate::CompareExchange {
            expected: accumulator.clone(),
            replacement,
        },
    )?;
    execution.write_flags(FlagChange::partial([(Flag::ZF, accumulator.eq(&previous))]))?;
    execution.write_register_pair::<I32>(Gpr32::Edx, Gpr32::Eax, previous)
}
