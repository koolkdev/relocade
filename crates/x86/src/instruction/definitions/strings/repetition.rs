//! String repetition owns the countdown and index progress between elements.

use wasm86_compiler::{Arguments, BuildError, Results, Val, I1, I32};

use crate::{execution::ExecutionBuilder, register::Gpr32};

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
    pub(super) fn execute<const N: usize>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        indices: [Gpr32; N],
        element: impl FnOnce(&mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        if matches!(self, Self::Once) {
            return element(execution);
        }
        repeat::<(), N>(
            execution,
            indices,
            (),
            |()| false.into(),
            |iteration, ()| element(iteration),
        )?;
        Ok(())
    }
}

/// Carries successful element results alongside count and index progress.
/// Each element receives the previous result so it can publish completed register
/// changes before another access can fault. New results and index changes must
/// follow successful accesses. The caller publishes the final returned result in
/// the parent. Returns the initial count and final payload.
pub(super) fn repeat<P: Results, const N: usize>(
    execution: &mut ExecutionBuilder<'_, '_>,
    indices: [Gpr32; N],
    initial: P::Values,
    done: impl FnOnce(&P::Values) -> Val<I1>,
    element: impl FnOnce(&mut ExecutionBuilder<'_, '_>, P::Values) -> Result<P::Values, BuildError>,
) -> Result<(Val<I32>, P::Values), BuildError>
where
    P::Values: Clone + Into<Arguments>,
{
    let count = execution.read_address_register(Gpr32::Ecx)?;
    let initial_indices = read_indices(execution, indices)?;
    let (remaining, final_indices, result) = execution.loop_until::<(I32, [I32; N], P)>(
        (count.clone(), initial_indices, initial),
        |(remaining, _, result)| remaining.eq(0).or(done(result)),
        |iteration, (remaining, positions, previous)| {
            iteration.write_address_register(Gpr32::Ecx, remaining.clone())?;
            write_indices(iteration, indices, positions)?;
            let result = element(iteration, previous)?;
            let next_indices = read_indices(iteration, indices)?;
            Ok((remaining.sub(1), next_indices, result))
        },
    )?;
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
