//! Counted string execution carries progress through a native structured loop.

use wasm86_compiler::{BuildError, Val, I32};

use crate::register::Gpr32;

use super::ExecutionBuilder;

/// The family adapter selects execution policy before generating its body.
pub(crate) enum Repetition {
    Once,
    Count,
}

impl ExecutionBuilder<'_, '_> {
    /// Executes elements that change only the supplied address-sized indices and
    /// guest memory. Each element must finish its faulting accesses before changing
    /// its indices. Fault publication then retains precisely the successful prefix
    /// of this instruction, without retiring the instruction itself.
    pub(crate) fn string_elements<const N: usize>(
        &mut self,
        repetition: Repetition,
        indices: [Gpr32; N],
        element: impl FnOnce(&mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        if matches!(repetition, Repetition::Once) {
            return element(self);
        }
        let count = self.read_address_register(Gpr32::Ecx)?;
        let initial_indices = self.read_indices(indices)?;
        let (remaining, final_indices) = self.loop_until::<(I32, [I32; N])>(
            (count, initial_indices),
            |(remaining, _)| remaining.eq(0),
            |iteration, (remaining, positions)| {
                iteration.write_address_register(Gpr32::Ecx, remaining.clone())?;
                iteration.write_indices(indices, positions)?;
                element(iteration)?;
                let next_indices = iteration.read_indices(indices)?;
                Ok((remaining.sub(1), next_indices))
            },
        )?;
        self.write_address_register(Gpr32::Ecx, remaining)?;
        self.write_indices(indices, final_indices)
    }

    fn read_indices<const N: usize>(
        &mut self,
        indices: [Gpr32; N],
    ) -> Result<[Val<I32>; N], BuildError> {
        let values = indices
            .into_iter()
            .map(|index| self.read_address_register(index))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values
            .try_into()
            .unwrap_or_else(|_| unreachable!("one value per index")))
    }

    fn write_indices<const N: usize>(
        &mut self,
        indices: [Gpr32; N],
        values: [Val<I32>; N],
    ) -> Result<(), BuildError> {
        for (index, value) in indices.into_iter().zip(values) {
            self.write_address_register(index, value)?;
        }
        Ok(())
    }
}
