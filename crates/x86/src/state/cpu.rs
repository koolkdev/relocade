//! CPU view of the shared execution context and its generated readers.

mod flags;
mod segments;

use std::cell::OnceCell;

use relocade_compiler::{BlockBuilder, BuildError, Func, Mem, Program, Val, I32};

use crate::flags::{Condition, StatusFlag};

use super::access::cpu_load;

pub(crate) struct Cpu {
    memory: Mem,
    condition_resolvers: [OnceCell<Func>; Condition::CANONICAL.len()],
    flag_resolvers: [OnceCell<Func>; 1 << StatusFlag::ALL.len()],
}

impl Cpu {
    pub(crate) fn declare(program: &mut Program) -> Self {
        let memory = program.import_memory(crate::abi::EXECUTION_CONTEXT);
        Self {
            memory,
            condition_resolvers: std::array::from_fn(|_| OnceCell::new()),
            flag_resolvers: std::array::from_fn(|_| OnceCell::new()),
        }
    }

    pub(crate) fn memory(&self) -> Mem {
        self.memory
    }

    pub(crate) fn read_eip(&self, body: &mut BlockBuilder<'_>) -> Result<Val<I32>, BuildError> {
        cpu_load!(body, self.memory, eip)
    }
}
