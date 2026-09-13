//! CPU backing and the generated readers owned by that backing.

mod flags;
mod segments;

use std::cell::Cell;

use wasm86_compiler::{BuildError, Func, FunctionBuilder, Mem, MemoryImport, Program, Val, I32};

use crate::flags::{Condition, StatusFlag};

use super::access::cpu_load;

pub(crate) struct Cpu {
    memory: Mem,
    condition_resolvers: [Cell<Option<Func>>; Condition::CANONICAL.len()],
    flag_resolvers: [Cell<Option<Func>>; 1 << StatusFlag::ALL.len()],
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
            flag_resolvers: std::array::from_fn(|_| Cell::new(None)),
        }
    }

    pub(crate) fn memory(&self) -> Mem {
        self.memory
    }

    pub(crate) fn read_eip(&self, body: &mut FunctionBuilder<'_>) -> Result<Val<I32>, BuildError> {
        cpu_load!(body, self.memory, eip)
    }
}
