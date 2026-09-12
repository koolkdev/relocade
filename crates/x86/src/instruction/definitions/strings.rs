//! Single string elements complete their accesses before advancing the indices.

use super::*;
use crate::{
    address::{Address32, RegisterTerm},
    alu::{AnyStatusSource, ArithmeticOp, StatusSource},
    flags::Flag,
    instruction::Location,
    register::{Gpr32, RegisterType},
};

instruction_families! {
    MOVS {
        execute: move_element;
        effects: [memory_read, memory_write];
        forms {
            0xA4 => byte();
            0xA5 => word_or_dword();
        }
    }
    CMPS {
        execute: compare_elements;
        effects: [memory_read];
        forms {
            0xA6 => byte();
            0xA7 => word_or_dword();
        }
    }
    STOS {
        execute: store_element;
        effects: [memory_write];
        forms {
            0xAA => byte();
            0xAB => word_or_dword();
        }
    }
    LODS {
        execute: load_element;
        effects: [memory_read];
        forms {
            0xAC => byte();
            0xAD => word_or_dword();
        }
    }
    SCAS {
        execute: scan_element;
        effects: [memory_read];
        forms {
            0xAE => byte();
            0xAF => word_or_dword();
        }
    }
}

fn move_element<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let value = memory_at_index::<T>(Gpr32::Esi).read(execution)?;
    memory_at_index::<T>(Gpr32::Edi).write(execution, value)?;
    advance_indices::<T>(execution, &[Gpr32::Esi, Gpr32::Edi])
}

fn compare_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let left = memory_at_index::<T>(Gpr32::Esi).read(execution)?;
    let right = memory_at_index::<T>(Gpr32::Edi).read(execution)?;
    execution.write_flags(ArithmeticOp::Subtract.apply(left, right).flags)?;
    advance_indices::<T>(execution, &[Gpr32::Esi, Gpr32::Edi])
}

fn store_element<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let value = TypedLocation::<T>::register(Gpr32::Eax).read(execution)?;
    memory_at_index::<T>(Gpr32::Edi).write(execution, value)?;
    advance_indices::<T>(execution, &[Gpr32::Edi])
}

fn load_element<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let value = memory_at_index::<T>(Gpr32::Esi).read(execution)?;
    TypedLocation::<T>::register(Gpr32::Eax).write(execution, value)?;
    advance_indices::<T>(execution, &[Gpr32::Esi])
}

fn scan_element<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let left = TypedLocation::<T>::register(Gpr32::Eax).read(execution)?;
    let right = memory_at_index::<T>(Gpr32::Edi).read(execution)?;
    execution.write_flags(ArithmeticOp::Subtract.apply(left, right).flags)?;
    advance_indices::<T>(execution, &[Gpr32::Edi])
}

fn memory_at_index<T: RegisterType>(index: Gpr32) -> TypedLocation<T> {
    TypedLocation::new(Location::Memory(Address32 {
        base: Some(RegisterTerm {
            register: index.into(),
            present: None,
        }),
        index: None,
        displacement: 0.into(),
    }))
}

fn advance_indices<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    indices: &[Gpr32],
) -> Result<(), BuildError> {
    let delta = execution
        .read_flag(Flag::DF)?
        .select(0u32.wrapping_sub(T::BYTES), T::BYTES);
    // Operand size changes the stride; indices retain the 32-bit address size.
    for &index in indices {
        let value = TypedLocation::<I32>::register(index).read(execution)?;
        TypedLocation::<I32>::register(index).write(execution, value.add(&delta))?;
    }
    Ok(())
}
