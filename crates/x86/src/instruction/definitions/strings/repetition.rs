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
        complete: impl Fn(
            &mut ExecutionBuilder<'_, '_>,
            &[StringOperand; N],
            &Val<I32>,
        ) -> Result<Val<I1>, BuildError>,
    ) -> Result<(), BuildError>
    where
        I32: wasm86_compiler::AtLeast<T>,
    {
        if matches!(self, Self::Once) {
            return element(execution, &operands);
        }
        let repeat = Repeat::<(), N>::new::<T>(execution, operands, ())?;
        let step = |iteration: &mut ExecutionBuilder<'_, '_>, (), operands: &[StringOperand; N]| {
            element(iteration, operands)
        };
        let done = |_: &()| false.into();
        repeat.execute(
            execution,
            |direct| {
                let completed = complete(direct, &repeat.resolved, &repeat.count)?;
                direct.if_value::<(I32, [I32; N], ())>(
                    completed,
                    |completed| {
                        let stride = super::element_stride::<T>(completed)?;
                        Ok((
                            0.into(),
                            std::array::from_fn(|index| {
                                repeat.initial_indices[index].add(repeat.count.mul(&stride))
                            }),
                            (),
                        ))
                    },
                    |scalar| repeat.loop_(scalar, &repeat.resolved, &done, &step),
                )
            },
            |checked| repeat.loop_(checked, &repeat.checked, &done, &step),
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
    let repeat = Repeat::<P, N>::new::<T>(execution, operands, initial)?;
    repeat.execute(
        execution,
        |direct| repeat.loop_(direct, &repeat.resolved, &done, &element),
        |checked| repeat.loop_(checked, &repeat.checked, &done, &element),
    )
}

/// Compiler transport for remaining count, address indices and carried results.
type LoopProgress<P, const N: usize> = (Val<I32>, [Val<I32>; N], <P as Results>::Values);

/// The entry proof and countdown values shared by scalar loops and complete transfers.
struct Repeat<P: Results, const N: usize> {
    count: Val<I32>,
    indices: [Gpr32; N],
    initial_indices: [Val<I32>; N],
    initial: P::Values,
    available: Val<I1>,
    resolved: [StringOperand; N],
    checked: [StringOperand; N],
    jit: bool,
}

impl<P: Results, const N: usize> Repeat<P, N>
where
    P::Values: Clone + Into<Arguments>,
{
    fn new<T: RegisterType>(
        execution: &mut ExecutionBuilder<'_, '_>,
        operands: [StringOperand; N],
        initial: P::Values,
    ) -> Result<Self, BuildError> {
        let count = execution.read_address_register(Gpr32::Ecx)?;
        let indices = std::array::from_fn(|index| operands[index].index);
        let initial_indices = read_indices(execution, indices)?;
        let (available, resolved) = execution.resolve_strings::<T, N>(&operands, &count)?;
        let jit = execution
            .specialize(|jit| jit.specialize_on(&available))?
            .is_some();
        Ok(Self {
            count,
            indices,
            initial_indices,
            initial,
            available,
            resolved,
            checked: operands,
            jit,
        })
    }

    fn loop_(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        operands: &[StringOperand; N],
        done: &impl Fn(&P::Values) -> Val<I1>,
        element: &impl Fn(
            &mut ExecutionBuilder<'_, '_>,
            P::Values,
            &[StringOperand; N],
        ) -> Result<P::Values, BuildError>,
    ) -> Result<LoopProgress<P, N>, BuildError> {
        execution.loop_until::<(I32, [I32; N], P)>(
            (
                self.count.clone(),
                self.initial_indices.clone(),
                self.initial.clone(),
            ),
            |(remaining, _, result)| remaining.eq(0).or(done(result)),
            |iteration, (remaining, positions, previous)| {
                iteration.write_address_register(Gpr32::Ecx, remaining.clone())?;
                write_indices(iteration, self.indices, positions)?;
                let result = element(iteration, previous, operands)?;
                let next_indices = read_indices(iteration, self.indices)?;
                Ok((remaining.sub(1), next_indices, result))
            },
        )
    }

    fn execute(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        direct: impl FnOnce(&mut ExecutionBuilder<'_, '_>) -> Result<LoopProgress<P, N>, BuildError>,
        checked: impl FnOnce(&mut ExecutionBuilder<'_, '_>) -> Result<LoopProgress<P, N>, BuildError>,
    ) -> Result<(Val<I32>, P::Values), BuildError> {
        // Only a JIT instruction boundary can hand off. The interpreter contains
        // the ordinary checked path beside the path using the complete range proof.
        let (remaining, final_indices, result) = if self.jit {
            direct(execution)?
        } else {
            execution.if_value::<(I32, [I32; N], P)>(&self.available, direct, checked)?
        };
        execution.write_address_register(Gpr32::Ecx, remaining)?;
        write_indices(execution, self.indices, final_indices)?;
        Ok((self.count.clone(), result))
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
