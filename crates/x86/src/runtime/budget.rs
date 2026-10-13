//! Slice scheduling state is separate from the architectural CPU image.

use crate::state::Cpu;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, MemoryImport, Program, Val, I32};

#[derive(Clone, Copy)]
pub(super) struct Budget(Mem);

impl Budget {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self(program.import_memory(MemoryImport {
            module: "wasm86".into(),
            name: "executionBudget".into(),
            minimum: 1,
            maximum: None,
            shared: false,
        }))
    }

    pub(super) fn remaining(self, body: &mut BlockBuilder<'_>) -> Result<Val<I32>, BuildError> {
        body.load_at(self.0, 0, 0)
    }

    /// Zero disables the retirement limit. The host records the starting counter
    /// separately so guest counter wrap does not change the requested allowance.
    pub(super) fn remaining_instructions(
        self,
        body: &mut BlockBuilder<'_>,
        cpu: &Cpu,
    ) -> Result<Val<I32>, BuildError> {
        let limit = body.load_at::<I32>(self.0, 4, 0)?;
        let start = body.load_at::<I32>(self.0, 8, 0)?;
        let count = cpu.read_instruction_count(body)?;
        Ok(limit.eq(0).select(u32::MAX, limit.sub(count.sub(start))))
    }

    pub(super) fn publish(
        self,
        body: &mut BlockBuilder<'_>,
        remaining: &Val<I32>,
    ) -> Result<(), BuildError> {
        body.store_at(self.0, 0, 0, remaining)
    }
}
