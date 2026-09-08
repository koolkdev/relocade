//! CPU backing and the generated readers owned by that backing.

mod conditions;

use std::cell::Cell;

use wasm86_compiler::{BuildError, Func, FunctionBuilder, Mem, MemoryImport, Program, Val, I32};

use crate::flags::Condition;

use super::EIP_OFFSET;

pub(crate) struct Cpu {
    memory: Mem,
    condition_resolvers: [Cell<Option<Func>>; Condition::CANONICAL.len()],
}

impl Cpu {
    pub(crate) fn declare(program: &mut Program) -> Self {
        Self {
            memory: program.import_memory(MemoryImport {
                module: "wasm86".into(),
                name: "cpuState".into(),
                minimum: 1,
                maximum: None,
            }),
            condition_resolvers: Condition::CANONICAL.map(|_| Cell::new(None)),
        }
    }

    pub(crate) fn memory(&self) -> Mem {
        self.memory
    }

    pub(crate) fn read_eip(&self, body: &mut FunctionBuilder<'_>) -> Result<Val<I32>, BuildError> {
        body.load(self.memory, EIP_OFFSET)
    }
}
