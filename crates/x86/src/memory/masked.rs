//! Byte selection reuses checked transfers without touching masked-off bytes.

use super::{Access, Intent, Memory};
use wasm86_compiler::{BlockBuilder, BuildError, MemoryType, Val, VectorType, I8};

impl Memory {
    pub(crate) fn write_masked<V: VectorType + MemoryType>(
        &self,
        body: &mut BlockBuilder<'_>,
        access: &Access,
        value: &Val<V>,
        mask: &Val<V>,
    ) -> Result<(), BuildError> {
        access.check_field::<V>(0);
        assert!(matches!(access.intent, Intent::Write));
        let bits = mask.bitmask::<I8>();
        for offset in 0..V::BYTES {
            body.if_(bits.and(1_u32 << offset).ne(0), |mut body| {
                self.write(&mut body, access, offset, &value.extract_lane::<I8>(offset))
            })?;
        }
        Ok(())
    }
}
