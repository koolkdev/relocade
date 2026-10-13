//! Bulk transfers require a permission proof for every physically consecutive page.

use super::Memory;
use wasm86_compiler::{BlockBuilder, BuildError, MemoryInt, Val, I32};

impl Memory {
    /// The complete source and destination spans must already be proven direct.
    pub(crate) fn copy(
        &self,
        body: &mut BlockBuilder<'_>,
        destination: &Val<I32>,
        source: &Val<I32>,
        bytes: &Val<I32>,
    ) -> Result<(), BuildError> {
        body.memory_copy(self.backing(), destination, self.backing(), source, bytes)
    }

    /// Repeats a complete little-endian element into a proven direct span.
    /// The byte count must be a positive multiple of the element width.
    pub(crate) fn fill<T: MemoryInt>(
        &self,
        body: &mut BlockBuilder<'_>,
        destination: &Val<I32>,
        value: &Val<T>,
        bytes: &Val<I32>,
    ) -> Result<(), BuildError>
    where
        I32: wasm86_compiler::AtLeast<T>,
    {
        let value32 = value.unsigned().extend::<I32>();
        if T::BYTES == 1 {
            return body.memory_fill(self.backing(), destination, value32, bytes);
        }
        let repeated_byte = value32.and(0xff).mul(if T::BYTES == 2 {
            0x101u32
        } else {
            0x101_0101u32
        });
        body.if_else(
            value32.eq(repeated_byte),
            |mut uniform| uniform.memory_fill(self.backing(), destination, &value32, bytes),
            |mut pattern| {
                // Double the initialized prefix. Each copy reads only completed bytes.
                pattern.store_at::<T>(self.backing(), destination, 0, value)?;
                pattern.loop_::<I32, ()>(T::BYTES, |mut iteration, labels, written| {
                    iteration.branch_if(written.eq(bytes), &labels.exit, ())?;
                    let remaining = bytes.sub(&written);
                    let length = written
                        .unsigned()
                        .lt(&remaining)
                        .select(&written, remaining);
                    self.copy(
                        &mut iteration,
                        &destination.add(&written),
                        destination,
                        &length,
                    )?;
                    iteration.branch(&labels.again, written.add(length))
                })
            },
        )
    }
}
