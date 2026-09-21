//! String elements complete their accesses before advancing the indices.

mod comparisons;
mod repetition;

use comparisons::{compare_elements, scan_elements, ComparisonRepetition};
use repetition::Repetition;

use super::*;
use crate::{
    flags::Flag,
    register::{Gpr32, RegisterType},
    segment::Segment,
};

instruction_families! {
    MOVS {
        execute: move_elements::<_>(Repetition::Once);
        effects: [memory_read, memory_write];
        repeat { F3 => move_elements::<_>(Repetition::Count); }
        forms {
            0xA4 => byte();
            0xA5 => word_or_dword();
        }
    }
    CMPS {
        execute: compare_elements::<_>(ComparisonRepetition::Once);
        effects: [memory_read];
        repeat {
            F3 => compare_elements::<_>(ComparisonRepetition::Equal);
            F2 => compare_elements::<_>(ComparisonRepetition::NotEqual);
        }
        forms {
            0xA6 => byte();
            0xA7 => word_or_dword();
        }
    }
    STOS {
        execute: store_elements::<_>(Repetition::Once);
        effects: [memory_write];
        repeat { F3 => store_elements::<_>(Repetition::Count); }
        forms {
            0xAA => byte();
            0xAB => word_or_dword();
        }
    }
    LODS {
        execute: load_element::<_>;
        effects: [memory_read];
        forms {
            0xAC => byte();
            0xAD => word_or_dword();
        }
    }
    SCAS {
        execute: scan_elements::<_>(ComparisonRepetition::Once);
        effects: [memory_read];
        repeat {
            F3 => scan_elements::<_>(ComparisonRepetition::Equal);
            F2 => scan_elements::<_>(ComparisonRepetition::NotEqual);
        }
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
    repetition.execute(execution, indices, |execution| {
        let value = execution
            .memory_at_register::<T>(Gpr32::Esi, execution.data_segment())
            .read(execution)?;
        execution
            .memory_at_register::<T>(Gpr32::Edi, Segment::Es.into())
            .write(execution, value)?;
        advance_indices(execution, &indices, &stride)
    })
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
    repetition.execute(execution, indices, |execution| {
        execution
            .memory_at_register::<T>(Gpr32::Edi, Segment::Es.into())
            .write(execution, &value)?;
        advance_indices(execution, &indices, &stride)
    })
}

fn load_element<T: RegisterType>(execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    let stride = element_stride::<T>(execution)?;
    let value = execution
        .memory_at_register::<T>(Gpr32::Esi, execution.data_segment())
        .read(execution)?;
    TypedLocation::<T>::register(Gpr32::Eax).write(execution, value)?;
    advance_indices(execution, &[Gpr32::Esi], &stride)
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
