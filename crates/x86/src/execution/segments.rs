//! Selector resolution and cache commitment are separate execution operations.

use wasm86_compiler::{BuildError, Val, I1, I16, I32};

use super::ExecutionBuilder;
use crate::{
    address::MemoryAddress, memory::Intent, register::RegisterType, segment::SegmentValues,
    Segment, SegmentDescriptorInfo,
};

/// A resolved segment cache and its destination, not yet installed in CPU state.
pub(crate) struct ResolvedSegment {
    segment: Segment,
    values: SegmentValues,
}

impl ResolvedSegment {
    pub(super) fn limit(&self) -> &Val<I32> {
        &self.values.limit
    }

    /// Installs the selector and cache after all instruction guards. Only
    /// completion and dispatch may follow if this breaks the entry's segment assumptions.
    pub(crate) fn commit(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
        execution
            .state
            .write_segment(&mut execution.body, &self.segment.into(), &self.values)
    }
}

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn query_segment_descriptor(
        &mut self,
        selector: &Val<I16>,
    ) -> Result<SegmentDescriptorInfo<Val<I1>, Val<I32>>, BuildError> {
        self.runtime
            .query_segment_descriptor(&mut self.body, selector)
    }

    pub(crate) fn read_segment_selector(
        &mut self,
        segment: Segment,
    ) -> Result<Val<I16>, BuildError> {
        self.state
            .read_segment_selector(&mut self.body, &segment.into())
    }

    /// Resolves the selector without installing its cache. Code offsets require
    /// a separate limit check before committing the resolved CS.
    pub(crate) fn resolve_segment(
        &mut self,
        segment: Segment,
        selector: &Val<I16>,
    ) -> Result<ResolvedSegment, BuildError> {
        let values = self.runtime.resolve_segment(
            &mut self.body,
            segment,
            selector,
            |body, exception| self.state.fault(body, &self.eip, self.completed, exception),
        )?;
        Ok(ResolvedSegment { segment, values })
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
