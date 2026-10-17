//! Generated cache reads and writes derive their positions from the backing schema.

use std::mem::size_of;

use wasm86_compiler::{BlockBuilder, BuildError, I8};

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
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
    ) -> Result<SegmentValues, BuildError> {
        let displacement = segment.index().mul(size_of::<StoredSegment>() as u32);
        Ok(SegmentValues {
            base: cpu_load!(body, self.memory, segments.es.base, at: &displacement)?,
            limit: cpu_load!(body, self.memory, segments.es.limit, at: &displacement)?,
            selector: cpu_load!(body, self.memory, segments.es.selector, at: &displacement)?,
            usable: cpu_load!(body, self.memory, segments.es.usable, at: &displacement)?.ne(0),
            readable: cpu_load!(body, self.memory, segments.es.readable, at: &displacement)?.ne(0),
            writable: cpu_load!(body, self.memory, segments.es.writable, at: &displacement)?.ne(0),
            executable: cpu_load!(body, self.memory, segments.es.executable, at: &displacement)?
                .ne(0),
            expand_down: cpu_load!(body, self.memory, segments.es.expand_down, at: &displacement)?
                .ne(0),
            big: cpu_load!(body, self.memory, segments.es.big, at: &displacement)?.ne(0),
        })
    }

    pub(crate) fn write_segment(
        &self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        values: &SegmentValues,
    ) -> Result<(), BuildError> {
        let displacement = segment.index().mul(size_of::<StoredSegment>() as u32);
        cpu_store!(body, self.memory, segments.es.base, &values.base, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.limit, &values.limit, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.selector, &values.selector, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.usable, values.usable.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.readable, values.readable.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.writable, values.writable.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.executable, values.executable.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.expand_down, values.expand_down.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.big, values.big.unsigned().extend::<I8>(), at: &displacement)
    }
}
