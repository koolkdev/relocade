//! Lazy segment fields initialized by the execution profile.

use std::mem::size_of;

use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I16};

use crate::{
    segment::{Segment, SegmentSelection, SegmentValues},
    ssa::StateFields,
    ExecutionProfile, SegmentDefaultSize, SegmentProfile,
};

use super::{
    access::{cpu_load, segment_location},
    Cpu, StoredSegment,
};

/// Retained fields agree with backing memory: segment writes commit immediately
/// and discard this cache. Cloning forks lazy reads for a descendant path.
#[derive(Clone)]
pub(crate) struct SegmentState<'cpu> {
    cpu: &'cpu Cpu,
    profile: ExecutionProfile,
    fields: StateFields,
}

impl<'cpu> SegmentState<'cpu> {
    /// Known fields are seeded as constants; construction emits no backing reads.
    pub(crate) fn new(cpu: &'cpu Cpu, profile: ExecutionProfile) -> Self {
        let mut fields = StateFields::new(cpu.memory());
        for segment in Segment::ALL {
            match profile {
                ExecutionProfile::Real16 => {
                    fields.assume(segment_location!(segment, limit), 0xffffu32);
                    fields.assume(segment_location!(segment, big), 0);
                }
                ExecutionProfile::Protected(SegmentProfile::Flat32)
                    if matches!(
                        segment,
                        Segment::Cs | Segment::Ds | Segment::Es | Segment::Ss
                    ) =>
                {
                    fields.assume(segment_location!(segment, base), 0);
                    fields.assume(segment_location!(segment, limit), u32::MAX);
                }
                ExecutionProfile::Protected(_) => continue,
            }
            fields.assume(segment_location!(segment, usable), 1);
            fields.assume(segment_location!(segment, readable), 1);
            fields.assume(
                segment_location!(segment, writable),
                u32::from(segment != Segment::Cs),
            );
            fields.assume(
                segment_location!(segment, executable),
                u32::from(segment == Segment::Cs),
            );
            fields.assume(segment_location!(segment, expand_down), 0);
        }
        fields.assume(
            segment_location!(Segment::Cs, big),
            u32::from(profile.code_default_size() == SegmentDefaultSize::Bits32),
        );
        if profile == ExecutionProfile::Protected(SegmentProfile::Flat32) {
            fields.assume(segment_location!(Segment::Ss, big), 1);
        }
        Self {
            cpu,
            profile,
            fields,
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
        let index = segment.index();
        let fixed_index = body.constant_bits(&index)?;
        let displacement = index.mul(size_of::<StoredSegment>() as u32);
        macro_rules! field {
            ($field:ident) => {{
                match fixed_index {
                    Some(index) => self.fields.read(body, segment_location!(index, $field))?,
                    None => {
                        let common = self.fields.common_value(
                            segment
                                .possible_segments()
                                .map(|segment| segment_location!(segment, $field)),
                        );
                        match common {
                            Some(value) => body.value(value)?,
                            None => cpu_load!(
                                body, self.cpu.memory(), segments.es.$field,
                                at: &displacement
                            )?,
                        }
                    }
                }
            }};
        }
        Ok(SegmentValues {
            base: field!(base),
            limit: field!(limit),
            selector: field!(selector),
            usable: field!(usable).ne(0),
            readable: field!(readable).ne(0),
            writable: field!(writable).ne(0),
            executable: field!(executable).ne(0),
            expand_down: field!(expand_down).ne(0),
            big: field!(big).ne(0),
        })
    }

    /// Compares the backing fields before replacing them with observed values.
    pub(crate) fn cs_matches(
        &mut self,
        body: &mut BlockBuilder<'_>,
        cache: &StoredSegment,
    ) -> Result<Val<I1>, BuildError> {
        Ok(self
            .fields
            .read(body, segment_location!(Segment::Cs, base))?
            .eq(cache.base)
            .and(
                self.fields
                    .read(body, segment_location!(Segment::Cs, limit))?
                    .eq(cache.limit),
            )
            .and(
                self.fields
                    .read(body, segment_location!(Segment::Cs, selector))?
                    .eq(u32::from(cache.selector)),
            )
            .and(
                self.fields
                    .read(body, segment_location!(Segment::Cs, usable))?
                    .eq(u32::from(cache.usable)),
            )
            .and(
                self.fields
                    .read(body, segment_location!(Segment::Cs, readable))?
                    .eq(u32::from(cache.readable)),
            )
            .and(
                self.fields
                    .read(body, segment_location!(Segment::Cs, writable))?
                    .eq(u32::from(cache.writable)),
            )
            .and(
                self.fields
                    .read(body, segment_location!(Segment::Cs, executable))?
                    .eq(u32::from(cache.executable)),
            )
            .and(
                self.fields
                    .read(body, segment_location!(Segment::Cs, expand_down))?
                    .eq(u32::from(cache.expand_down)),
            )
            .and(
                self.fields
                    .read(body, segment_location!(Segment::Cs, big))?
                    .eq(u32::from(cache.big)),
            ))
    }

    /// Retains CS values after the block-entry specialization guard succeeds.
    pub(crate) fn assume_cs(&mut self, cache: &StoredSegment) {
        self.fields
            .assume(segment_location!(Segment::Cs, base), cache.base);
        self.fields
            .assume(segment_location!(Segment::Cs, limit), cache.limit);
        self.fields.assume(
            segment_location!(Segment::Cs, selector),
            u32::from(cache.selector),
        );
        self.fields.assume(
            segment_location!(Segment::Cs, usable),
            u32::from(cache.usable),
        );
        self.fields.assume(
            segment_location!(Segment::Cs, readable),
            u32::from(cache.readable),
        );
        self.fields.assume(
            segment_location!(Segment::Cs, writable),
            u32::from(cache.writable),
        );
        self.fields.assume(
            segment_location!(Segment::Cs, executable),
            u32::from(cache.executable),
        );
        self.fields.assume(
            segment_location!(Segment::Cs, expand_down),
            u32::from(cache.expand_down),
        );
        self.fields
            .assume(segment_location!(Segment::Cs, big), u32::from(cache.big));
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
