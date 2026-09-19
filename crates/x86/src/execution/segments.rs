//! Segment instructions keep resolution separate from cache commitment.

use wasm86_compiler::{BuildError, Val, I16};

use super::ExecutionBuilder;
use crate::{segment::SegmentValues, Segment};

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

    fn resolve_segment(
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
