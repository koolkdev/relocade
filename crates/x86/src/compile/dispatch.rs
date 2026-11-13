//! Address resolution shared by every kind of callable execution entry.

mod real_mode;
mod table;
pub use table::EntryTable;

use relocade_compiler::{BlockBuilder, BuildError, Mem, Program, Table, Val, I32};

use crate::{
    state::{Cpu, SegmentReads},
    CompiledModule, ExecutionProfile, Segment,
};

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
    /// Compiles profile-specific lookup through `relocade.executionFunctions`.
    /// A hit tail-calls its slot; a miss tail-calls slot one. The generated code
    /// has no host-call import or instruction decoder. Real16 first checks
    /// address-bound overrides, then looks up compiled blocks by packed CS:IP.
    /// The embedding maintains the execution profile. Each destination owns its
    /// entry checks; lookup does not check instruction bytes or CS permissions.
    /// Only Real16 records unresolved calls, using packed 16:16 CS:IP keys.
    /// Protected-mode dispatch does not collect call profiles.
    pub fn compile_dispatcher(&self) -> Result<CompiledModule, BuildError> {
        let mut program = Program::new();
        let cpu = Arc::new(Cpu::declare(&mut program));
        let dispatch = Dispatch::declare(&mut program);
        let profile = self.target.profile;
        let lookup = match profile {
            ExecutionProfile::Real16 => {
                let lookup = real_mode::lookup(&mut program, cpu.clone(), dispatch);
                real_mode::define_call_resolver(&mut program, cpu, lookup)?;
                lookup
            }
            ExecutionProfile::Protected(_) => {
                let lookup = program.function::<fn() -> u32>(move |mut body, ()| {
                    let eip = cpu.read_eip(&mut body)?;
                    let mut segments = SegmentReads::new(&cpu, profile);
                    let base = segments.base(&mut body, &Segment::Cs.into())?;
                    dispatch.lookup(body, &base.add(eip))
                });
                crate::abi::RESOLVE_CALL.define(&mut program, move |mut body, ()| {
                    let slot = lookup.call(&mut body, ())?;
                    body.return_((slot.ne(0).select(slot, EntryTable::INTERPRETER_SLOT), true))
                })?;
                lookup
            }
        };
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
