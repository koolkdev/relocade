//! Guest dispatch and call resolution for the active execution profile.

mod real_mode;
mod table;
pub use table::EntryTable;

use relocade_compiler::{BlockBuilder, BuildError, Mem, Program, Table, Val, I32};

use crate::{state::Cpu, CompiledModule, ExecutionProfile, SegmentProfile};

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
    /// Dispatches to an installed entry or the interpreter. Real16 uses packed CS:IP
    /// and records unresolved calls. Flat32 uses the full linear instruction address.
    ///
    /// # Panics
    /// Segmented protected-mode dispatch is not yet supported.
    pub fn compile_dispatcher(&self) -> Result<CompiledModule, BuildError> {
        let mut program = Program::new();
        let cpu = Arc::new(Cpu::declare(&mut program));
        let dispatch = Dispatch::declare(&mut program);
        let (lookup, resolve_call) = match self.target.profile {
            ExecutionProfile::Real16 => {
                let lookup = real_mode::lookup(&mut program, cpu.clone(), dispatch);
                let resolve = real_mode::define_call_resolver(&mut program, cpu, lookup)?;
                (lookup, resolve)
            }
            ExecutionProfile::Protected(SegmentProfile::Flat32) => {
                let lookup = program.function::<fn() -> u32>(move |mut body, ()| {
                    let eip = cpu.read_eip(&mut body)?;
                    dispatch.lookup(body, &eip)
                });
                let resolve =
                    crate::abi::RESOLVE_CALL.define(&mut program, move |mut body, ()| {
                        let slot = lookup.call(&mut body, ())?;
                        let slot = slot.ne(0).select(slot, EntryTable::INTERPRETER_SLOT);
                        body.return_((slot, true))
                    })?;
                (lookup, resolve)
            }
            ExecutionProfile::Protected(_) => panic!("segmented protected-mode dispatch"),
        };
        crate::abi::RESOLVE_AND_JUMP.define(&mut program, move |mut body, ()| {
            let (slot, can_continue) = resolve_call.call(&mut body, ())?;
            body.if_(can_continue.eq(false), |body| {
                body.return_(crate::ExecutionExitValues::call_profile_full())
            })?;
            dispatch.enter(body, slot)
        })?;
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
