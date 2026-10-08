//! Instruction retirement and event delivery own interrupt-inhibition publication.

use super::*;

#[derive(Clone, Default)]
pub(crate) struct InterruptState {
    shadow: Option<Val<I1>>,
    pending: Option<Val<I1>>,
}

impl InterruptState {
    pub(crate) fn inhibit(&mut self, condition: impl Into<Val<I1>>) {
        self.pending = Some(condition.into());
    }

    /// Ordinary retirement expires an incoming shadow without reading its value.
    /// A successful inhibiting instruction replaces it with its pending value.
    pub(crate) fn retire(&mut self) {
        self.shadow = Some(self.pending.take().unwrap_or(false.into()));
    }

    pub(crate) fn clear(&mut self) {
        self.shadow = Some(false.into());
        self.pending = None;
    }

    pub(crate) fn publish(&self, body: &mut BlockBuilder<'_>, cpu: &Cpu) -> Result<(), BuildError> {
        if let Some(shadow) = &self.shadow {
            cpu_store!(
                body,
                cpu.memory(),
                interrupt_shadow,
                shadow.unsigned().extend::<I8>()
            )?;
        }
        Ok(())
    }
}
