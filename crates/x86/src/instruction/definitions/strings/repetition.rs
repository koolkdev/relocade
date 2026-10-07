//! String repetition owns the countdown and index progress between elements.

use wasm86_compiler::{Arguments, BuildError, Results, Val, I1, I32};

use crate::{
    execution::{ExecutionBuilder, StringOperand},
    register::{Gpr32, RegisterType},
};

/// Whether a string instruction runs once or consumes its address-sized count.
pub(super) enum Repetition {
    Once,
    Count,
}

impl Repetition {
    /// Executes elements that change only the supplied address-sized indices and
    /// guest memory. Each element must finish its faulting accesses before changing
    /// its indices. Fault publication then retains precisely the successful prefix
    /// of this instruction, without retiring the instruction itself.
    pub(super) fn execute<T: RegisterType, const N: usize>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        operands: [StringOperand; N],
        element: impl Fn(&mut ExecutionBuilder<'_, '_>, &[StringOperand; N]) -> Result<(), BuildError>,
    ) -> Result<(), BuildError>
    where
        I32: wasm86_compiler::AtLeast<T>,
    {
        if matches!(self, Self::Once) {
            return element(execution, &operands);
        }
        repeat::<T, (), N>(
            execution,
            operands,
            (),
            |()| false.into(),
            |iteration, (), operands| element(iteration, operands),
        )?;
        Ok(())
    }
}

/// Carries successful element results alongside count and index progress.
/// Each element receives the previous result so it can publish completed register
/// changes before another access can fault. New results and index changes must
/// follow successful accesses. The caller publishes the final returned result in
/// the parent. Returns the initial count and final payload.
pub(super) fn repeat<T: RegisterType, P: Results, const N: usize>(
    execution: &mut ExecutionBuilder<'_, '_>,
    operands: [StringOperand; N],
    initial: P::Values,
    done: impl Fn(&P::Values) -> Val<I1>,
    element: impl Fn(
        &mut ExecutionBuilder<'_, '_>,
        P::Values,
        &[StringOperand; N],
    ) -> Result<P::Values, BuildError>,
) -> Result<(Val<I32>, P::Values), BuildError>
where
    P::Values: Clone + Into<Arguments>,
    I32: wasm86_compiler::AtLeast<T>,
{
    let count = execution.read_address_register(Gpr32::Ecx)?;
    let indices = std::array::from_fn(|index| operands[index].index);
    let initial_indices = read_indices(execution, indices)?;
    let (available, resolved) = execution.resolve_strings::<T, N>(&operands, &count)?;
    let run = |execution: &mut ExecutionBuilder<'_, '_>, operands: &[StringOperand; N]| {
        execution.loop_until::<(I32, [I32; N], P)>(
            (count.clone(), initial_indices.clone(), initial.clone()),
            |(remaining, _, result)| remaining.eq(0).or(done(result)),
            |iteration, (remaining, positions, previous)| {
                iteration.write_address_register(Gpr32::Ecx, remaining.clone())?;
                write_indices(iteration, indices, positions)?;
                let result = element(iteration, previous, operands)?;
                let next_indices = read_indices(iteration, indices)?;
                Ok((remaining.sub(1), next_indices, result))
            },
        )
    };
    // Only an instruction-boundary JIT path can hand off. Nested regions and
    // the interpreter contain the checked loop alongside the resolved loop.
    let jit = execution
        .specialize(|jit| {
            jit.specialize_on(&available)?;
            Ok(())
        })?
        .is_some();
    let (remaining, final_indices, result) = if jit {
        run(execution, &resolved)?
    } else {
        execution.if_value::<(I32, [I32; N], P)>(
            available,
            |direct| run(direct, &resolved),
            |checked| run(checked, &operands),
        )?
    };
    execution.write_address_register(Gpr32::Ecx, remaining)?;
    write_indices(execution, indices, final_indices)?;
    Ok((count, result))
}

fn read_indices<const N: usize>(
    execution: &mut ExecutionBuilder<'_, '_>,
    indices: [Gpr32; N],
) -> Result<[Val<I32>; N], BuildError> {
    let values = indices
        .into_iter()
        .map(|index| execution.read_address_register(index))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values
        .try_into()
        .unwrap_or_else(|_| unreachable!("one value per index")))
}

fn write_indices<const N: usize>(
    execution: &mut ExecutionBuilder<'_, '_>,
    indices: [Gpr32; N],
    values: [Val<I32>; N],
) -> Result<(), BuildError> {
    for (index, value) in indices.into_iter().zip(values) {
        execution.write_address_register(index, value)?;
    }
    Ok(())
}
