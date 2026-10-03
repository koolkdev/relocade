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
        memory: [StringOperand; N],
        stride: &Val<I32>,
        operands: impl Fn(
            &mut ExecutionBuilder<'_, '_>,
            &[StringOperand; N],
        ) -> Result<[Val<T>; 2], BuildError>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
        StatusSource<T>: Into<AnyStatusSource>,
    {
        let indices: [Gpr32; N] = std::array::from_fn(|index| memory[index].index);
        if matches!(self, Self::Once) {
            let [left, right] = operands(execution, &memory)?;
            execution.write_flags(ArithmeticOp::Subtract.apply(left, right).flags)?;
            return advance_indices(execution, &indices, stride);
        }

        let (count, (_, [left, right])) = repetition::repeat::<T, (I1, [T; 2]), N>(
            execution,
            memory,
            (false.into(), [0.into(), 0.into()]),
            |(done, _)| done.clone(),
            |iteration, _, memory| {
                let [left, right] = operands(iteration, memory)?;
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
    let memory = [
        StringOperand::new(Gpr32::Esi, execution.data_segment(), Intent::Read),
        StringOperand::new(Gpr32::Edi, Segment::Es.into(), Intent::Read),
    ];
    repetition.execute(execution, memory, &stride, |execution, memory| {
        let left = memory[0].read::<T>(execution)?;
        let right = memory[1].read::<T>(execution)?;
        Ok([left.clone(), right])
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
    let memory = [StringOperand::new(
        Gpr32::Edi,
        Segment::Es.into(),
        Intent::Read,
    )];
    repetition.execute(execution, memory, &stride, |execution, memory| {
        let right = memory[0].read::<T>(execution)?;
        Ok([left.clone(), right])
    })
}
