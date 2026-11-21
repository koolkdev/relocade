//! Generated cache writes derive their positions from the backing schema.

use std::mem::size_of;

use relocade_compiler::{BlockBuilder, BuildError, I8};

use crate::{
    segment::{SegmentCacheValues, SegmentSelection},
    state::{access::cpu_store, SegmentRegister},
};

use super::Cpu;

impl Cpu {
    pub(crate) fn write_segment(
        &self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        values: &SegmentCacheValues,
    ) -> Result<(), BuildError> {
        let displacement = segment.index().mul(size_of::<SegmentRegister>() as u32);
        cpu_store!(body, self.memory, segments.es.base, &values.base, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.limit, &values.properties.limit, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.selector, &values.selector, at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.usable, values.properties.usable.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.readable, values.properties.readable.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.writable, values.properties.writable.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.executable, values.properties.executable.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.expand_down, values.properties.expand_down.unsigned().extend::<I8>(), at: &displacement)?;
        cpu_store!(body, self.memory, segments.es.big, values.properties.big.unsigned().extend::<I8>(), at: &displacement)
    }
}
