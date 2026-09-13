//! String elements complete their accesses before advancing the indices.

use super::*;
use crate::{
    address::{EffectiveAddress, MemoryAddress, RegisterTerm},
    alu::{AnyStatusSource, ArithmeticOp, StatusSource},
    execution::Repetition,
    flags::Flag,
    instruction::Location,
    register::{Gpr32, RegisterType},
    segment::{Segment, SegmentSelection},
};

instruction_families! {
    MOVS {
        execute: move_elements(Repetition::Once);
        effects: [memory_read, memory_write];
        repeat: move_elements(Repetition::Count);
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
        execute: store_elements(Repetition::Once);
        effects: [memory_write];
        repeat: store_elements(Repetition::Count);
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

fn move_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    repetition: Repetition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let indices = [Gpr32::Esi, Gpr32::Edi];
    let stride = element_stride::<T>(execution)?;
    execution.string_elements(repetition, indices, |execution| {
        let value = memory_at_index::<T>(execution, Gpr32::Esi, execution.string_source_segment())
            .read(execution)?;
        memory_at_index::<T>(execution, Gpr32::Edi, Segment::Es.into()).write(execution, value)?;
        advance_indices(execution, &indices, &stride)
    })
}

fn compare_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let stride = element_stride::<T>(execution)?;
    let left = memory_at_index::<T>(execution, Gpr32::Esi, execution.string_source_segment())
        .read(execution)?;
    let right = memory_at_index::<T>(execution, Gpr32::Edi, Segment::Es.into()).read(execution)?;
    execution.write_flags(ArithmeticOp::Subtract.apply(left, right).flags)?;
    advance_indices(execution, &[Gpr32::Esi, Gpr32::Edi], &stride)
}

fn store_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    repetition: Repetition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let indices = [Gpr32::Edi];
    let stride = element_stride::<T>(execution)?;
    let value = TypedLocation::<T>::register(Gpr32::Eax).read(execution)?;
    execution.string_elements(repetition, indices, |execution| {
        memory_at_index::<T>(execution, Gpr32::Edi, Segment::Es.into()).write(execution, &value)?;
        advance_indices(execution, &indices, &stride)
    })
}

fn load_element<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let stride = element_stride::<T>(execution)?;
    let value = memory_at_index::<T>(execution, Gpr32::Esi, execution.string_source_segment())
        .read(execution)?;
    TypedLocation::<T>::register(Gpr32::Eax).write(execution, value)?;
    advance_indices(execution, &[Gpr32::Esi], &stride)
}

fn scan_element<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let stride = element_stride::<T>(execution)?;
    let left = TypedLocation::<T>::register(Gpr32::Eax).read(execution)?;
    let right = memory_at_index::<T>(execution, Gpr32::Edi, Segment::Es.into()).read(execution)?;
    execution.write_flags(ArithmeticOp::Subtract.apply(left, right).flags)?;
    advance_indices(execution, &[Gpr32::Edi], &stride)
}

fn memory_at_index<T: RegisterType>(
    execution: &ExecutionBuilder<'_, '_>,
    index: Gpr32,
    segment: SegmentSelection,
) -> TypedLocation<T> {
    TypedLocation::new(Location::Memory(
        MemoryAddress {
            segment,
            offset: EffectiveAddress {
                size: execution.address_size(),
                base: Some(RegisterTerm {
                    register: index.into(),
                    present: None,
                }),
                index: None,
                displacement: 0.into(),
            },
        }
        .into(),
    ))
}

fn element_stride<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
) -> Result<Val<I32>, BuildError> {
    Ok(execution
        .read_flag(Flag::DF)?
        .select(0u32.wrapping_sub(T::BYTES), T::BYTES))
}

fn advance_indices(
    execution: &mut ExecutionBuilder<'_, '_>,
    indices: &[Gpr32],
    stride: &Val<I32>,
) -> Result<(), BuildError> {
    // Operand size changes the stride; address size selects the register alias.
    for &index in indices {
        let value = execution.read_address_register(index)?;
        execution.write_address_register(index, value.add(stride))?;
    }
    Ok(())
}
