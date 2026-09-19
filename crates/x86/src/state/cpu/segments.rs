//! Generated cache reads and writes derive their positions from the backing schema.

use std::mem::size_of;

use wasm86_compiler::{BuildError, FunctionBuilder};

use crate::{
    segment::{SegmentSelection, SegmentValues},
    state::{
        access::{cpu_load, cpu_store},
        StoredSegment,
    },
};

use super::Cpu;

impl Cpu {
    pub(crate) fn read_segment(
        &self,
        body: &mut FunctionBuilder<'_>,
        segment: &SegmentSelection,
    ) -> Result<SegmentValues, BuildError> {
        let displacement = segment.index().mul(size_of::<StoredSegment>() as u32);
        Ok(SegmentValues {
            base: cpu_load!(body, self.memory, segments.es.base, at: &displacement)?,
            limit: cpu_load!(body, self.memory, segments.es.limit, at: &displacement)?,
            selector: cpu_load!(body, self.memory, segments.es.selector, at: &displacement)?,
            attributes: cpu_load!(body, self.memory, segments.es.attributes, at: &displacement)?,
        })
    }

    pub(crate) fn write_segment(
        &self,
        body: &mut FunctionBuilder<'_>,
        segment: &SegmentSelection,
        values: &SegmentValues,
    ) -> Result<(), BuildError> {
        let displacement = segment.index().mul(size_of::<StoredSegment>() as u32);
        cpu_store!(body, self.memory, segments.es.base, &values.base, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.limit, &values.limit, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.selector, &values.selector, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.attributes, &values.attributes, at: &displacement)
    }
}
