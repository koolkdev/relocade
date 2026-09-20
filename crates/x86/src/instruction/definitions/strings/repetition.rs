//! String repetition owns the countdown and index progress between elements.

use wasm86_compiler::{BuildError, Val, I32};

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
        let count = execution.read_address_register(Gpr32::Ecx)?;
        let initial_indices = read_indices(execution, indices)?;
        let (remaining, final_indices) = execution.loop_until::<(I32, [I32; N])>(
            (count, initial_indices),
            |(remaining, _)| remaining.eq(0),
            |iteration, (remaining, positions)| {
                iteration.write_address_register(Gpr32::Ecx, remaining.clone())?;
                write_indices(iteration, indices, positions)?;
                element(iteration)?;
                let next_indices = read_indices(iteration, indices)?;
                Ok((remaining.sub(1), next_indices))
            },
        )?;
        execution.write_address_register(Gpr32::Ecx, remaining)?;
        write_indices(execution, indices, final_indices)
    }
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
