//! Selector resolution and cache commitment are separate execution operations.

use relocade_compiler::{BuildError, Val, I1, I16, I32};

use super::ExecutionBuilder;
use crate::{
    address::MemoryAddress, memory::Intent, register::RegisterType, segment::SegmentValues,
    ExecutionProfile, Segment, SegmentDescriptorInfoValues,
};

/// A resolved segment cache and its destination, not yet installed in CPU state.
pub(crate) struct ResolvedSegment {
    segment: Segment,
    values: SegmentValues,
    needs_completion: Option<Val<I1>>,
}

impl ResolvedSegment {
    pub(super) fn limit(&self) -> &Val<I32> {
        &self.values.limit
    }

    /// Installs the selector and cache after all instruction guards. Only
    /// completion and dispatch may follow.
    pub(crate) fn commit(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
        if let Some(needs_completion) = self.needs_completion {
            execution.runtime.complete_segment_load(
                &mut execution.body,
                &self.values.selector,
                needs_completion,
            )?;
        }
        execution
            .state
            .write_segment(&mut execution.body, &self.segment.into(), &self.values)
    }
}

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn query_segment_descriptor(
        &mut self,
        selector: &Val<I16>,
    ) -> Result<SegmentDescriptorInfoValues, BuildError> {
        self.runtime
            .query_segment_descriptor(&mut self.body, selector)
    }

    pub(crate) fn read_segment_selector(
        &mut self,
        segment: Segment,
    ) -> Result<Val<I16>, BuildError> {
        self.segments.read_selector(&mut self.body, &segment.into())
    }

    /// Resolves the selector without installing its cache. Code offsets require
    /// a separate limit check before committing the resolved CS.
    pub(crate) fn resolve_segment(
        &mut self,
        segment: Segment,
        selector: &Val<I16>,
    ) -> Result<ResolvedSegment, BuildError> {
        let (values, needs_completion) = match self.profile() {
            ExecutionProfile::Real16 => {
                // Real16 admits only canonical caches. Retained descriptor limits
                // and attributes across mode transitions require a different profile.
                let cache = crate::SegmentRegister::real_mode(segment, 0);
                (
                    SegmentValues {
                        base: selector.unsigned().extend::<I32>().shl(4),
                        limit: cache.limit.into(),
                        selector: selector.clone(),
                        usable: (cache.usable != 0).into(),
                        readable: (cache.readable != 0).into(),
                        writable: (cache.writable != 0).into(),
                        executable: (cache.executable != 0).into(),
                        expand_down: (cache.expand_down != 0).into(),
                        big: (cache.big != 0).into(),
                    },
                    None,
                )
            }
            ExecutionProfile::Protected(_) => {
                let resolved = self.runtime.resolve_segment(
                    &mut self.body,
                    segment,
                    selector,
                    |body, exception| self.state.fault(body, &self.eip, self.completed, exception),
                )?;
                (resolved.cache, Some(resolved.needs_completion))
            }
        };
        Ok(ResolvedSegment {
            segment,
            values,
            needs_completion,
        })
    }

    /// Reads an offset followed by a selector through the entry address and cache.
    pub(crate) fn read_far_pointer<T: RegisterType>(
        &mut self,
        source: MemoryAddress<Val<I32>>,
    ) -> Result<(Val<T>, Val<I16>), BuildError> {
        // Address size wraps the starting offset. The selector follows the offset
        // inside one complete operand span, including across a 16-bit boundary.
        let operand = self.memory_operand(source, T::BYTES + 2, Intent::Read, &[])?;
        let pointer_offset = operand.read::<T>(self, 0)?;
        let selector = operand.read::<I16>(self, T::BYTES)?;
        Ok((pointer_offset, selector))
    }
}
