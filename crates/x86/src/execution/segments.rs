//! Segment instructions keep resolution separate from cache commitment.

use wasm86_compiler::{BuildError, Val, I16, I32};

use super::ExecutionBuilder;
use crate::{
    address::{self, MemoryAddress},
    memory::Intent,
    register::{Register, RegisterType},
    segment::SegmentValues,
    Segment,
};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn read_segment_selector(
        &mut self,
        segment: Segment,
    ) -> Result<Val<I16>, BuildError> {
        self.state
            .read_segment_selector(&mut self.body, &segment.into())
    }

    pub(crate) fn load_segment(
        &mut self,
        segment: Segment,
        selector: Val<I16>,
    ) -> Result<(), BuildError> {
        let values = self.resolve_segment(segment, &selector)?;
        self.state
            .write_segment(&mut self.body, &segment.into(), &values)
    }

    pub(crate) fn pop_segment(
        &mut self,
        segment: Segment,
        slot_bytes: u32,
    ) -> Result<(), BuildError> {
        let popped = self.read_stack::<I16>(slot_bytes)?;
        let values = self.resolve_segment(segment, popped.value())?;
        // Resolve before changing ESP; commit its old-SS pointer before replacing
        // the cache, since the new SS may have a different base or stack width.
        popped.commit(self, 0)?;
        self.state
            .write_segment(&mut self.body, &segment.into(), &values)
    }

    pub(crate) fn load_far_pointer<T: RegisterType>(
        &mut self,
        segment: Segment,
        destination: Register<T>,
        source: MemoryAddress<Val<I32>>,
    ) -> Result<(), BuildError> {
        let (pointer_offset, selector) = self.read_far_pointer::<T>(source)?;
        let values = self.resolve_segment(segment, &selector)?;
        self.state
            .write_register(&mut self.body, destination, pointer_offset)?;
        self.state
            .write_segment(&mut self.body, &segment.into(), &values)
    }

    /// Reads an offset followed by a selector through the entry address and cache.
    pub(crate) fn read_far_pointer<T: RegisterType>(
        &mut self,
        source: MemoryAddress<Val<I32>>,
    ) -> Result<(Val<T>, Val<I16>), BuildError> {
        let offset = address::resolve(&mut self.body, &mut self.state, source.offset, &[])?;
        let memory = self
            .memory
            .expect("a far pointer source declares guest memory");
        // Address size wraps the starting offset. The selector follows the offset
        // inside one complete operand span, including across a 16-bit boundary.
        let access = self.checked(memory, &source.segment, &offset, T::BYTES + 2, Intent::Read)?;
        let pointer_offset = memory.read::<T>(&mut self.body, &access, 0)?;
        let selector = memory.read::<I16>(&mut self.body, &access, T::BYTES)?;
        Ok((pointer_offset, selector))
    }

    pub(super) fn resolve_segment(
        &mut self,
        segment: Segment,
        selector: &Val<I16>,
    ) -> Result<SegmentValues, BuildError> {
        self.runtime
            .resolve_segment(&mut self.body, segment, selector, |body, exception| {
                self.state.fault(body, &self.eip, self.completed, exception)
            })
    }
}
