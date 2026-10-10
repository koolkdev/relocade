//! Each port element completes its transfer before publishing index/count progress.

use super::*;

pub(super) fn input_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    repetition: Repetition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    execution.interpret_tracked_memory()?;
    let port = TypedLocation::<I16>::register(Gpr32::Edx).read(execution)?;
    let stride = element_stride::<T>(execution)?;
    repetition.execute::<T, 1>(
        execution,
        [StringOperand::new(Gpr32::Edi, Segment::Es.into(), Intent::Write).with_live_routing()],
        |execution, operands| {
            operands[0].write_from(execution, |execution| execution.read_port::<T>(&port))?;
            advance_indices(execution, &[Gpr32::Edi], &stride)
        },
        |_, _, _| Ok(false.into()),
    )
}

pub(super) fn output_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    repetition: Repetition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
{
    execution.interpret_tracked_memory()?;
    let port = TypedLocation::<I16>::register(Gpr32::Edx).read(execution)?;
    let stride = element_stride::<T>(execution)?;
    repetition.execute::<T, 1>(
        execution,
        [
            StringOperand::new(Gpr32::Esi, execution.data_segment(), Intent::Read)
                .with_live_routing(),
        ],
        |execution, operands| {
            let value = operands[0].read::<T>(execution)?;
            execution.write_port(&port, &value)?;
            advance_indices(execution, &[Gpr32::Esi], &stride)
        },
        |_, _, _| Ok(false.into()),
    )
}
