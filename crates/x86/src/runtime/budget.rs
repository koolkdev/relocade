//! Slice scheduling state is separate from the architectural CPU image.

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

    pub(super) fn publish(
        self,
        body: &mut BlockBuilder<'_>,
        remaining: &Val<I32>,
    ) -> Result<(), BuildError> {
        body.store_at(self.0, 0, 0, remaining)
    }
}
