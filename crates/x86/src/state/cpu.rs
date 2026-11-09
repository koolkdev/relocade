//! CPU view of the shared execution context and its generated readers.

mod flags;
mod segments;

use relocade_compiler::{BlockBuilder, BuildError, Func, Mem, Program, Val, I32};

use crate::flags::{Condition, FlagMask, StatusFlag};
use flags::FlagQuery;

use super::access::cpu_load;

pub(crate) struct Cpu {
    memory: Mem,
    condition_resolvers: [Func; Condition::CANONICAL.len()],
    flag_resolvers: [Func; (1 << StatusFlag::ALL.len()) - 1],
}

impl Cpu {
    pub(crate) fn declare(program: &mut Program) -> Self {
        let memory = program.import_memory(crate::abi::EXECUTION_CONTEXT);
        Self {
            memory,
            condition_resolvers: Condition::CANONICAL
                .map(|condition| FlagQuery::Condition(condition).declare(program, memory)),
            flag_resolvers: std::array::from_fn(|index| {
                let mask = StatusFlag::ALL
                    .into_iter()
                    .filter(|flag| (index + 1) & (1 << *flag as usize) != 0)
                    .fold(FlagMask::EMPTY, |mask, flag| mask.union(FlagMask::of(flag)));
                FlagQuery::Flags(mask).declare(program, memory)
            }),
        }
    }

    pub(crate) fn memory(&self) -> Mem {
        self.memory
    }

    pub(crate) fn read_eip(&self, body: &mut BlockBuilder<'_>) -> Result<Val<I32>, BuildError> {
        cpu_load!(body, self.memory, eip)
    }
}
