//! Real-mode dispatch and call resolution use packed CS:IP entries.

mod real_mode;
mod table;
pub use table::EntryTable;

use relocade_compiler::{BlockBuilder, BuildError, Mem, Program, Table, Val, I32};

use crate::{state::Cpu, CompiledModule, ExecutionProfile};

use super::Compiler;
use std::sync::Arc;

/// Generated access to shared callable slots.
#[derive(Clone, Copy)]
pub(super) struct Dispatch {
    memory: Mem,
    functions: Table,
}

impl Dispatch {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self {
            memory: program.import_memory(crate::abi::EXECUTION_ENTRIES),
            functions: program.import_table(crate::abi::EXECUTION_FUNCTIONS),
        }
    }

    pub(super) fn enter(
        &self,
        body: BlockBuilder<'_>,
        slot: impl Into<Val<I32>>,
    ) -> Result<(), BuildError> {
        body.tail_call_indirect(self.functions, slot, &[])
    }
}

impl Compiler {
    /// Compiles lookup of packed Real16 CS:IP entries. Hooks, compiled blocks and
    /// rejected candidates share one map. Misses enter the interpreter; unresolved
    /// calls also contribute to profiling. Destinations own their entry checks.
    ///
    /// # Panics
    /// Panics for a target other than Real16. Protected-mode dispatch is not yet supported.
    pub fn compile_dispatcher(&self) -> Result<CompiledModule, BuildError> {
        assert_eq!(self.target.profile, ExecutionProfile::Real16);
        let mut program = Program::new();
        let cpu = Arc::new(Cpu::declare(&mut program));
        let dispatch = Dispatch::declare(&mut program);
        let lookup = real_mode::lookup(&mut program, cpu.clone(), dispatch);
        real_mode::define_call_resolver(&mut program, cpu, lookup)?;
        crate::abi::DISPATCH.define(&mut program, move |mut body, ()| {
            let slot = lookup.call(&mut body, ())?;
            let slot = slot.ne(0).select(slot, EntryTable::INTERPRETER_SLOT);
            dispatch.enter(body, slot)
        })?;
        crate::abi::INTERPRET.define(&mut program, move |body, ()| {
            dispatch.enter(body, EntryTable::INTERPRETER_SLOT)
        })?;
        Ok(CompiledModule {
            bytes: program.compile()?,
            entry: crate::abi::DISPATCH.name.into(),
            execution_target: Some(self.target),
        })
    }
}
