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
        forms {
            0xA4 => byte();
            0xA5 => word_or_dword();
        }
    }
    REP_MOVS {
        execute: move_elements::<_>(Repetition::Count);
        effects: [memory_read, memory_write];
        forms {
            F3 0xA4 => byte();
            F3 0xA5 => word_or_dword();
        }
    }
    CMPS {
        execute: compare_elements::<_>(ComparisonRepetition::Once);
        effects: [memory_read];
        forms {
            0xA6 => byte();
            0xA7 => word_or_dword();
        }
    }
    REPE_CMPS {
        execute: compare_elements::<_>(ComparisonRepetition::Equal);
        effects: [memory_read];
        forms {
            F3 0xA6 => byte();
            F3 0xA7 => word_or_dword();
        }
    }
    REPNE_CMPS {
        execute: compare_elements::<_>(ComparisonRepetition::NotEqual);
        effects: [memory_read];
        forms {
            F2 0xA6 => byte();
            F2 0xA7 => word_or_dword();
        }
    }
    STOS {
        execute: store_elements::<_>(Repetition::Once);
        effects: [memory_write];
        forms {
            0xAA => byte();
            0xAB => word_or_dword();
        }
    }
    REP_STOS {
        execute: store_elements::<_>(Repetition::Count);
        effects: [memory_write];
        forms {
            F3 0xAA => byte();
            F3 0xAB => word_or_dword();
        }
    }
    LODS {
        execute: load_elements::<_>(Repetition::Once);
        effects: [memory_read];
        forms {
            0xAC => byte();
            0xAD => word_or_dword();
        }
    }
    REP_LODS {
        execute: load_elements::<_>(Repetition::Count);
        effects: [memory_read];
        forms {
            F3 0xAC => byte();
            F3 0xAD => word_or_dword();
        }
    }
    SCAS {
        execute: scan_elements::<_>(ComparisonRepetition::Once);
        effects: [memory_read];
        forms {
            0xAE => byte();
            0xAF => word_or_dword();
        }
    }
    REPE_SCAS {
        execute: scan_elements::<_>(ComparisonRepetition::Equal);
        effects: [memory_read];
        forms {
            F3 0xAE => byte();
            F3 0xAF => word_or_dword();
        }
    }
    REPNE_SCAS {
        execute: scan_elements::<_>(ComparisonRepetition::NotEqual);
        effects: [memory_read];
        forms {
            F2 0xAE => byte();
            F2 0xAF => word_or_dword();
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

fn load_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    repetition: Repetition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    if matches!(repetition, Repetition::Once) {
        return load_element::<T>(execution);
    }
    let initial = TypedLocation::<T>::register(Gpr32::Eax).read(execution)?;
    let (_, value) = repetition::repeat::<T, 1>(
        execution,
        [Gpr32::Esi],
        initial,
        |_| false.into(),
        |iteration, previous| {
            // A later fault must retain the last completed load.
            TypedLocation::<T>::register(Gpr32::Eax).write(iteration, previous)?;
            load_element::<T>(iteration)?;
            TypedLocation::<T>::register(Gpr32::Eax).read(iteration)
        },
    )?;
    TypedLocation::<T>::register(Gpr32::Eax).write(execution, value)
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
