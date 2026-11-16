//! Complete string spans permit relative accesses within a repeated element loop.

mod bulk;

use relocade_compiler::{BuildError, Val, I1, I32};

use super::ExecutionBuilder;
use crate::{
    flags::Flag,
    memory::{Access, Intent},
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

/// String operands whose direct backing may be used when the range is available.
pub(crate) struct ResolvedStrings<const N: usize> {
    pub(crate) available: Val<I1>,
    pub(crate) operands: [StringOperand; N],
}

#[derive(Clone)]
struct RelativeRange {
    offset_start: Val<I32>,
    access: Access,
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
        I32: relocade_compiler::AtLeast<T>,
    {
        match &self.relative {
            Some(range) => {
                let position = execution.read_address_register(self.index)?;
                let access = range.access.field::<T>(position.sub(&range.offset_start))?;
                execution
                    .accesses
                    .memory()
                    .read::<T>(&mut execution.body, &access, 0)
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
        I32: relocade_compiler::AtLeast<T>,
    {
        match &self.relative {
            Some(range) => {
                let position = execution.read_address_register(self.index)?;
                let access = range.access.field::<T>(position.sub(&range.offset_start))?;
                execution
                    .accesses
                    .memory()
                    .write(&mut execution.body, &access, 0, value)
            }
            None => execution
                .memory_at_register::<T>(self.index, self.segment.clone())
                .write(execution, value),
        }
    }

    /// Checks the destination before reading the device, then stores the result.
    /// Physical routing remains live across the device callback.
    pub(crate) fn write_from_device<T: RegisterType>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        read_device: impl FnOnce(&mut ExecutionBuilder<'_, '_>) -> Result<Val<T>, BuildError>,
    ) -> Result<(), BuildError>
    where
        I32: relocade_compiler::AtLeast<T>,
    {
        assert!(matches!(self.intent, Intent::Write));
        match &self.relative {
            Some(range) => {
                let position = execution.read_address_register(self.index)?;
                let access = range.access.field::<T>(position.sub(&range.offset_start))?;
                let value = read_device(execution)?;
                let memory = execution.accesses.memory();
                let access = memory.refresh_access(&mut execution.body, &access)?;
                memory.write(&mut execution.body, &access, 0, &value)
            }
            None => {
                let address =
                    execution.memory_address_at_register(self.index, self.segment.clone());
                let mut target = execution.memory_operand(address, T::BYTES, self.intent, &[])?;
                let value = read_device(execution)?;
                target.refresh_routing(execution)?;
                target.write(execution, 0, value)
            }
        }
    }
}

impl ExecutionBuilder<'_, '_> {
    /// Non-faulting preflight. Zero count skips all operand checks. Oversized or
    /// wrapping offset spans, denied permissions and scattered backing need the
    /// checked interpreter loop to determine actual progress and fault priority.
    /// Physical JIT spans must fit one direct page; the interpreter keeps live
    /// per-element routing. Callers using device callbacks cannot retain this proof.
    pub(crate) fn resolve_strings<T: RegisterType, const N: usize>(
        &mut self,
        operands: &[StringOperand; N],
        count: &Val<I32>,
    ) -> Result<Option<ResolvedStrings<N>>, BuildError> {
        if !self.accesses.memory().has_stable_mappings() && !self.can_fallback {
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
            eligible = eligible.and(self.address_mask.unsigned().ge(&last));
            starts.push(start);
        }
        let empty = count.eq(0);
        let memory = self.accesses.memory();
        let mut resolved = Vec::with_capacity(N);
        for (operand, start) in operands.iter().zip(starts) {
            let segment = self.segments.check(
                &mut self.body,
                &operand.segment,
                &start,
                &bytes,
                operand.intent,
            )?;
            let access = memory.check_access(
                &mut self.body,
                &segment.linear,
                &bytes,
                operand.intent,
                Some(
                    eligible
                        .eq(false)
                        .or(&empty)
                        .or(segment.denied.unwrap_or(false.into())),
                ),
                None,
            )?;
            resolved.push(StringOperand {
                relative: Some(RelativeRange {
                    offset_start: start,
                    access,
                }),
                ..operand.clone()
            });
        }
        let mut resolved: [StringOperand; N] =
            resolved.try_into().unwrap_or_else(|_| unreachable!());
        let available = self.guard_accesses(
            empty.eq(false),
            resolved
                .each_mut()
                .map(|operand| &mut operand.relative.as_mut().unwrap().access),
        )?;
        for operand in &mut resolved {
            let access = &mut operand.relative.as_mut().unwrap().access;
            // Relative operands are used only on the complete-span success path.
            access.denied = false.into();
            access.unavailable = false.into();
        }
        Ok(Some(ResolvedStrings {
            available,
            operands: resolved,
        }))
    }
}
