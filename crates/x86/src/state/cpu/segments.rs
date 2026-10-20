//! Generated cache writes derive their positions from the backing schema.

use std::mem::size_of;

use relocade_compiler::{BlockBuilder, BuildError, I8};

use crate::{
    segment::{SegmentSelection, SegmentValues},
    state::{access::cpu_store, SegmentRegister},
};

use super::Cpu;

impl Cpu {
    pub(crate) fn write_segment(
        &self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        values: &SegmentValues,
    ) -> Result<(), BuildError> {
        let displacement = segment.index().mul(size_of::<SegmentRegister>() as u32);
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
