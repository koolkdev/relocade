//! Lazy segment fields for one execution path, refined by its entry profile.

use std::mem::{offset_of, size_of};

use wasm86_compiler::{BlockBuilder, BuildError, Val, I16};

use crate::{
    segment::{Segment, SegmentSelection, SegmentValues},
    ssa::{Location, SsaType, StateFields},
    ExecutionProfile,
};

use super::{Cpu, ExecutionContext, StoredSegment};

/// Segment-changing instructions finish the current path. Cloning forks lazy
/// reads for a descendant path, just as for the other architectural state.
#[derive(Clone)]
pub(crate) struct SegmentState<'cpu> {
    cpu: &'cpu Cpu,
    profile: ExecutionProfile,
    fields: StateFields,
}

impl<'cpu> SegmentState<'cpu> {
    pub(crate) fn new(cpu: &'cpu Cpu, profile: ExecutionProfile) -> Self {
        Self {
            cpu,
            profile,
            fields: StateFields::new(cpu.memory()),
        }
    }

    pub(crate) fn profile(&self) -> ExecutionProfile {
        self.profile
    }

    pub(crate) fn read(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
    ) -> Result<SegmentValues, BuildError> {
        if let Some(values) = body.map_constant_choices(
            segment.index(),
            |body, index| {
                Segment::ALL
                    .get(index as usize)
                    .map(|&segment| self.read_named(body, segment))
                    .transpose()
            },
            SegmentValues::select,
        )? {
            return Ok(values);
        }
        self.cpu.read_segment(body, segment)
    }

    fn read_named(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: Segment,
    ) -> Result<SegmentValues, BuildError> {
        let assumptions = self.profile.segment_assumptions(segment);
        Ok(SegmentValues {
            base: self.fields.read_assuming(
                body,
                location(segment, offset_of!(StoredSegment, base)),
                |value| assumptions.base.map_or(value, Into::into),
            )?,
            limit: self.fields.read_assuming(
                body,
                location(segment, offset_of!(StoredSegment, limit)),
                |value| assumptions.limit.map_or(value, Into::into),
            )?,
            selector: self
                .fields
                .read(body, location(segment, offset_of!(StoredSegment, selector)))?,
            attributes: self.fields.read_assuming(
                body,
                location(segment, offset_of!(StoredSegment, attributes)),
                |value| {
                    value
                        .and(u32::from(!assumptions.attribute_mask))
                        .or(u32::from(assumptions.attribute_value))
                },
            )?,
        })
    }

    /// The visible selector remains readable when the loaded cache is unusable.
    pub(crate) fn read_selector(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
    ) -> Result<Val<I16>, BuildError> {
        Ok(self.read(body, segment)?.selector)
    }

    /// Commits a resolved cache after all instruction guards. Only completion
    /// and dispatch may follow when this breaks the entry's profile assumptions.
    pub(crate) fn write(
        &mut self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        values: &SegmentValues,
    ) -> Result<(), BuildError> {
        self.cpu.write_segment(body, segment, values)?;
        self.fields = StateFields::new(self.cpu.memory());
        Ok(())
    }
}

fn location<T: SsaType>(segment: Segment, field: usize) -> Location<T> {
    Location::new(
        (offset_of!(ExecutionContext, cpu.segments)
            + segment as usize * size_of::<StoredSegment>()
            + field) as u32,
    )
}
