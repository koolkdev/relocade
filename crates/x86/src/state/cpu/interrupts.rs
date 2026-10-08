use wasm86_compiler::{BlockBuilder, BuildError, Val, I1};

use super::{cpu_load, Cpu};

impl Cpu {
    /// Tests the published Real16 state at a host interrupt boundary.
    pub(crate) fn accepts_maskable_interrupt(
        &self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        let enabled = cpu_load!(body, self.memory, flags.bytes.if_)?.truncate::<I1>();
        let inhibited = cpu_load!(body, self.memory, interrupt_shadow)?.truncate::<I1>();
        Ok(enabled.and(inhibited.eq(false)))
    }
}
