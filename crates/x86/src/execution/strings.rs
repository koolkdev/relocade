//! Complete string spans permit relative accesses within a repeated element loop.

mod bulk;

use wasm86_compiler::{BuildError, Val, I1, I32};

use super::ExecutionBuilder;
use crate::{
    address::AddressSize,
    flags::Flag,
    memory::Intent,
    register::{Gpr32, RegisterType},
    segment::SegmentSelection,
};

/// An implicit string operand, optionally backed by a dominating range proof.
/// Its proof is scoped to one repetition; each complete element must remain
/// within the checked span.
#[derive(Clone)]
pub(crate) struct StringOperand {
    pub(crate) index: Gpr32,
    segment: SegmentSelection,
    intent: Intent,
    relative: Option<RelativeRange>,
}

#[derive(Clone)]
struct RelativeRange {
    offset_start: Val<I32>,
    physical_start: Val<I32>,
}

impl StringOperand {
    pub(crate) fn new(index: Gpr32, segment: SegmentSelection, intent: Intent) -> Self {
        Self {
            index,
            segment,
            intent,
            relative: None,
        }
    }

    pub(crate) fn read<T: RegisterType>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<Val<T>, BuildError>
    where
        I32: wasm86_compiler::AtLeast<T>,
    {
        match &self.relative {
            Some(range) => {
                let position = execution.read_address_register(self.index)?;
                let memory = execution.memory.as_ref().unwrap().memory();
                memory.load(
                    &mut execution.body,
                    &range.physical_start.add(position.sub(&range.offset_start)),
                    0,
                )
            }
            None => execution
                .memory_at_register::<T>(self.index, self.segment.clone())
                .read(execution),
        }
    }

    pub(crate) fn write<T: RegisterType>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        value: &Val<T>,
    ) -> Result<(), BuildError>
    where
        I32: wasm86_compiler::AtLeast<T>,
    {
        assert!(matches!(self.intent, Intent::Write));
        match &self.relative {
            Some(range) => {
                let position = execution.read_address_register(self.index)?;
                let memory = execution.memory.as_ref().unwrap().memory();
                memory.store(
                    &mut execution.body,
                    &range.physical_start.add(position.sub(&range.offset_start)),
                    value,
                )
            }
            None => execution
                .memory_at_register::<T>(self.index, self.segment.clone())
                .write(execution, value),
        }
    }
}

impl ExecutionBuilder<'_, '_> {
    /// Non-faulting preflight. Zero count skips all operand checks. Oversized or
    /// wrapping offset spans, denied permissions and scattered backing need the
    /// checked interpreter loop to determine actual progress and fault priority.
    pub(crate) fn resolve_strings<T: RegisterType, const N: usize>(
        &mut self,
        operands: &[StringOperand; N],
        count: &Val<I32>,
    ) -> Result<(Val<I1>, [StringOperand; N]), BuildError> {
        let bytes = count.mul(T::BYTES);
        let backward = self.read_flag(Flag::DF)?;
        let mut eligible = self
            .body
            .value::<I32>(u32::MAX / T::BYTES)?
            .unsigned()
            .ge(count);
        let mut starts = Vec::with_capacity(N);
        for operand in operands {
            let position = self.read_address_register(operand.index)?;
            let start = backward.select(position.sub(bytes.sub(T::BYTES)), &position);
            let last = start.add(bytes.sub(1));
            eligible = eligible
                .and(position.unsigned().ge(&start))
                .and(last.unsigned().ge(&position));
            if self.address_size == AddressSize::Bits16 {
                eligible = eligible.and(last.unsigned().lt(0x10000u32));
            }
            starts.push(start);
        }
        let starts: [Val<I32>; N] = starts.try_into().unwrap_or_else(|_| unreachable!());
        let memory = self.memory.as_ref().unwrap().memory();
        let (available, physical) = self.body.if_value::<(I1, [I32; N])>(
            count.eq(0),
            |zero| zero.yield_((true, [0u32; N])),
            |mut nonempty| {
                let result = nonempty.if_value::<(I1, [I32; N])>(
                    eligible,
                    |mut checks| {
                        let mut available: Val<I1> = true.into();
                        let mut physical = Vec::with_capacity(N);
                        for (operand, start) in operands.iter().zip(&starts) {
                            let segment = self.segments.check(
                                &mut checks,
                                &operand.segment,
                                start,
                                &bytes,
                                operand.intent,
                            )?;
                            let (direct, address) = checks.if_value::<(I1, I32)>(
                                segment.denied.unwrap_or(false.into()),
                                |denied| denied.yield_((false, 0)),
                                |mut paging| {
                                    let access = memory.resolve_access(
                                        &mut paging,
                                        &segment.linear,
                                        &bytes,
                                        operand.intent,
                                        None,
                                        None,
                                    )?;
                                    paging.yield_((access.unavailable.eq(false), access.physical))
                                },
                            )?;
                            available = available.and(direct);
                            physical.push(address);
                        }
                        let physical: [Val<I32>; N] =
                            physical.try_into().unwrap_or_else(|_| unreachable!());
                        checks.yield_((available, physical))
                    },
                    |ineligible| ineligible.yield_((false, [0u32; N])),
                )?;
                nonempty.yield_(result)
            },
        )?;
        let resolved = std::array::from_fn(|index| StringOperand {
            relative: Some(RelativeRange {
                offset_start: starts[index].clone(),
                physical_start: physical[index].clone(),
            }),
            ..operands[index].clone()
        });
        Ok((available, resolved))
    }
}
