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
    live_routing: bool,
}

/// String operands whose direct backing may be used when the range is available.
pub(crate) struct ResolvedStrings<const N: usize> {
    pub(crate) available: Val<I1>,
    pub(crate) operands: [StringOperand; N],
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
            live_routing: false,
        }
    }

    /// Device callbacks can remap this operand between transfers.
    pub(crate) fn with_live_routing(mut self) -> Self {
        self.live_routing = true;
        self
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
        self.write_from(execution, |_| Ok(value.clone()))
    }

    /// Checks the destination before producing a value with observable effects,
    /// then writes it. Physical routing remains live across that callback.
    pub(crate) fn write_from<T: RegisterType>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        read: impl FnOnce(&mut ExecutionBuilder<'_, '_>) -> Result<Val<T>, BuildError>,
    ) -> Result<(), BuildError>
    where
        I32: wasm86_compiler::AtLeast<T>,
    {
        assert!(matches!(self.intent, Intent::Write));
        match &self.relative {
            Some(range) => {
                let position = execution.read_address_register(self.index)?;
                let address = range.physical_start.add(position.sub(&range.offset_start));
                let value = read(execution)?;
                let memory = execution.memory.as_ref().unwrap().memory();
                memory.store(&mut execution.body, &address, &value)
            }
            None => {
                let target = execution
                    .memory_at_register::<T>(self.index, self.segment.clone())
                    .prepare_write(execution, &[])?;
                let value = read(execution)?;
                target.write(execution, value)
            }
        }
    }
}

impl ExecutionBuilder<'_, '_> {
    /// Caps a budgeted chunk at operand page boundaries. A straddling element
    /// gets a one-element chunk, whose ordinary range proof may still succeed.
    pub(crate) fn repetition_chunk<T: RegisterType, const N: usize>(
        &mut self,
        count: &Val<I32>,
        operands: &[StringOperand; N],
    ) -> Result<Val<I32>, BuildError> {
        let Some(work) = &self.work else {
            return Ok(count.clone());
        };
        let mut chunk = count.unsigned().lt(work).select(count, work);
        let backward = self.read_flag(Flag::DF)?;
        for operand in operands {
            let position = self.read_address_register(operand.index)?;
            let segment = self.segments.check(
                &mut self.body,
                &operand.segment,
                &position,
                T::BYTES,
                operand.intent,
            )?;
            let offset = segment.linear.and(4095);
            let forward = Val::<I32>::from(4096)
                .sub(&offset)
                .unsigned()
                .shr(T::BYTES.trailing_zeros());
            let reverse = offset.unsigned().shr(T::BYTES.trailing_zeros()).add(1);
            let elements = backward.select(reverse, forward);
            let elements = elements.eq(0).select(1, &elements);
            chunk = chunk.unsigned().lt(&elements).select(chunk, elements);
        }
        self.body.value(chunk)
    }

    /// Non-faulting preflight. Zero count skips all operand checks. Oversized or
    /// wrapping offset spans, denied permissions and scattered backing need the
    /// checked interpreter loop to determine actual progress and fault priority.
    /// Bounded physical chunks require direct RAM/ROM so no device callback can
    /// invalidate the proof. Unbudgeted physical repetitions retain checked accesses.
    pub(crate) fn resolve_strings<T: RegisterType, const N: usize>(
        &mut self,
        operands: &[StringOperand; N],
        count: &Val<I32>,
    ) -> Result<Option<ResolvedStrings<N>>, BuildError> {
        if operands.iter().any(|operand| operand.live_routing)
            || (!self.runtime.is_budgeted()
                && !self.memory.as_ref().unwrap().memory().has_stable_mappings())
        {
            return Ok(None);
        }
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
                                    let access = memory.check_direct_access(
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
        Ok(Some(ResolvedStrings {
            available,
            operands: resolved,
        }))
    }
}
