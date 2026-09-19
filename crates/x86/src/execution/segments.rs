//! Segment instructions keep resolution separate from cache commitment.

use wasm86_compiler::{BuildError, Val, I16};

use super::ExecutionBuilder;
use crate::Segment;

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
        let values = self.runtime.resolve_segment(
            &mut self.body,
            segment,
            &selector,
            |body, exception| self.state.fault(body, &self.eip, self.completed, exception),
        )?;
        self.state
            .write_segment(&mut self.body, &segment.into(), &values)
    }
}
