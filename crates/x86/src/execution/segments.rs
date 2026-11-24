//! Selector resolution and cache commitment are separate execution operations.

use relocade_compiler::{
    abi::{AbiType, CompilerValue},
    BuildError, Val, I16, I32,
};

use super::{transfer::Transfer, ExecutionBuilder};
use crate::{
    address::MemoryAddress,
    memory::Intent,
    register::RegisterType,
    segment::cache::{SegmentResolution, SegmentResolutionValues},
    Segment, SegmentDescriptorInfoValues,
};

/// A resolved segment cache and its destination, not yet installed in CPU state.
pub(crate) struct ResolvedSegment {
    segment: Segment,
    resolution: SegmentResolutionValues,
}

impl ResolvedSegment {
    pub(super) fn limit(&self) -> &Val<I32> {
        &self.resolution.cache.properties.limit
    }

    /// Installs the selector and cache after all instruction guards. Only
    /// completion and dispatch may follow.
    pub(crate) fn commit(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
        let real_mode = execution.real_mode()?;
        if execution.body.constant_bits(&real_mode)? != Some(1) {
            // Only a known real-mode reload preserves the cache layout for local links.
            execution.transfer = Transfer::Dispatch;
        }
        if let Some(descriptors) = execution.runtime.descriptors {
            descriptors.complete_segment_load(
                &mut execution.body,
                &self.resolution.cache.selector,
                self.resolution.needs_completion,
            )?;
        }
        execution.state.write_segment(
            &mut execution.body,
            &self.segment.into(),
            &self.resolution.cache,
        )
    }
}

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn query_segment_descriptor(
        &mut self,
        selector: &Val<I16>,
    ) -> Result<Option<SegmentDescriptorInfoValues>, BuildError> {
        self.runtime
            .descriptors
            .map(|descriptors| descriptors.query_segment_descriptor(&mut self.body, selector))
            .transpose()
    }

    pub(crate) fn read_segment_selector(
        &mut self,
        segment: Segment,
    ) -> Result<Val<I16>, BuildError> {
        self.state
            .segments()
            .read_selector(&mut self.body, &segment.into())
    }

    /// Resolves the selector without installing its cache. Code offsets require
    /// a separate limit check before committing the resolved CS.
    pub(crate) fn resolve_segment(
        &mut self,
        segment: Segment,
        selector: &Val<I16>,
    ) -> Result<ResolvedSegment, BuildError> {
        type Fields = <SegmentResolution as AbiType>::Logical;
        let real = SegmentResolutionValues::real_mode(segment, selector);
        let protected = self.protected_mode()?;
        let resolution = if let Some(descriptors) = self.runtime.descriptors {
            let fields = self.body.block::<Fields>(|mut body, _| {
                body.yield_if(protected.eq(false), real)?;
                let resolved = descriptors.resolve_segment(
                    &mut body,
                    segment,
                    selector,
                    |body, exception| self.state.fault(body, &self.eip, self.completed, exception),
                )?;
                body.yield_(resolved)
            })?;
            SegmentResolution::from_values(fields)
        } else {
            self.unsupported_if(protected)?;
            real
        };
        Ok(ResolvedSegment {
            segment,
            resolution,
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
