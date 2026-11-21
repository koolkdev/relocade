//! Selector resolution and cache commitment are separate execution operations.

use relocade_compiler::{BuildError, Val, I1, I16, I32};

use super::{transfer::Transfer, ExecutionBuilder};
use crate::{
    address::MemoryAddress, memory::Intent, register::RegisterType, segment::SegmentValues,
    Segment, SegmentDescriptorInfoValues,
};

/// A resolved segment cache and its destination, not yet installed in CPU state.
pub(crate) struct ResolvedSegment {
    segment: Segment,
    values: SegmentValues,
    needs_completion: Val<I1>,
}

impl ResolvedSegment {
    pub(super) fn limit(&self) -> &Val<I32> {
        &self.values.limit
    }

    /// Installs the selector and cache after all instruction guards. Only
    /// completion and dispatch may follow.
    pub(crate) fn commit(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
        let real_mode = execution.real_mode()?;
        if execution.body.constant_bits(&real_mode)? != Some(1) {
            // Only a known real-mode reload preserves the cache layout for local links.
            execution.transfer = Transfer::Dispatch;
        }
        execution.runtime.complete_segment_load(
            &mut execution.body,
            &self.values.selector,
            self.needs_completion,
        )?;
        execution
            .state
            .write_segment(&mut execution.body, &self.segment.into(), &self.values)
    }
}

impl ExecutionBuilder<'_, '_> {
    /// A real-mode load can construct its cache directly from the selector.
    pub(crate) fn guard_descriptor_load(&mut self, opcode: u8) -> Result<(), BuildError> {
        if !self.runtime.supports_protected_descriptors() {
            let protected = self.protected_mode()?;
            self.unsupported_if(protected, u32::from(opcode))?;
        }
        Ok(())
    }

    /// Queries are protected-only. Instruction admission has already emitted
    /// the real-mode #UD before reaching this backend check.
    pub(crate) fn admit_descriptor_query(&mut self, opcode: u8) -> Result<bool, BuildError> {
        let supported = self.runtime.supports_protected_descriptors();
        if !supported {
            self.unsupported_if(true, u32::from(opcode))?;
        }
        Ok(supported)
    }

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
        let protected = self.protected_mode()?;
        let resolved = self.runtime.resolve_segment(
            &mut self.body,
            protected,
            segment,
            selector,
            |body, exception| self.state.fault(body, &self.eip, self.completed, exception),
        )?;
        Ok(ResolvedSegment {
            segment,
            values: resolved.cache,
            needs_completion: resolved.needs_completion,
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
