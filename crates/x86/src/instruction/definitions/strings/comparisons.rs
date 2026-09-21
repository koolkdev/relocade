//! String comparisons publish their last flags only on successful completion.

use super::*;
use crate::alu::{AnyStatusSource, ArithmeticOp, StatusSource};
use wasm86_compiler::I1;

pub(super) enum ComparisonRepetition {
    Once,
    Equal,
    NotEqual,
}

impl ComparisonRepetition {
    fn execute<T: RegisterType, const N: usize>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        indices: [Gpr32; N],
        stride: &Val<I32>,
        operands: impl FnOnce(&mut ExecutionBuilder<'_, '_>) -> Result<[Val<T>; 2], BuildError>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
        StatusSource<T>: Into<AnyStatusSource>,
    {
        if matches!(self, Self::Once) {
            let [left, right] = operands(execution)?;
            execution.write_flags(ArithmeticOp::Subtract.apply(left, right).flags)?;
            return advance_indices(execution, &indices, stride);
        }

        let (count, (_, [left, right])) = repetition::repeat::<(I1, [T; 2]), N>(
            execution,
            indices,
            (false.into(), [0.into(), 0.into()]),
            |(done, _)| done.clone(),
            |iteration| {
                let [left, right] = operands(iteration)?;
                advance_indices(iteration, &indices, stride)?;
                let done = match self {
                    Self::Equal => left.ne(&right),
                    Self::NotEqual => left.eq(&right),
                    Self::Once => unreachable!(),
                };
                Ok((done, [left, right]))
            },
        )?;
        // Intel REP specifies entry EFLAGS on a CMPS/SCAS fault. The loop leaves
        // flags untouched; only a successful nonempty repetition replaces them.
        // Intel SDM Vol. 2B, REP/REPE/REPZ/REPNE/REPNZ, pp. 4-250–252.
        execution.write_flags(
            ArithmeticOp::Subtract
                .apply(left, right)
                .flags
                .when(count.ne(0)),
        )
    }
}

pub(super) fn compare_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    repetition: ComparisonRepetition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let stride = element_stride::<T>(execution)?;
    repetition.execute(execution, [Gpr32::Esi, Gpr32::Edi], &stride, |execution| {
        let left = execution
            .memory_at_register::<T>(Gpr32::Esi, execution.data_segment())
            .read(execution)?;
        let right = execution
            .memory_at_register::<T>(Gpr32::Edi, Segment::Es.into())
            .read(execution)?;
        Ok([left, right])
    })
}

pub(super) fn scan_elements<T: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    repetition: ComparisonRepetition,
) -> Result<(), BuildError>
where
    I32: AtLeast<T>,
    StatusSource<T>: Into<AnyStatusSource>,
{
    let stride = element_stride::<T>(execution)?;
    let left = TypedLocation::<T>::register(Gpr32::Eax).read(execution)?;
    repetition.execute(execution, [Gpr32::Edi], &stride, |execution| {
        let right = execution
            .memory_at_register::<T>(Gpr32::Edi, Segment::Es.into())
            .read(execution)?;
        Ok([left, right])
    })
}
