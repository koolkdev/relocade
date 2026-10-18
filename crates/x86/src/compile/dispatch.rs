//! Address resolution shared by every kind of callable execution entry.

mod table;
pub use table::EntryTable;

use wasm86_compiler::{BlockBuilder, BuildError, Mem, Program, Table, Val, I32};

use crate::{
    state::{Cpu, SegmentReads},
    CompiledModule, Segment,
};

use super::Compiler;

/// Generated access to shared callable slots.
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
    /// Compiles uniform address dispatch through `wasm86.executionFunctions`.
    /// A hit tail-calls its slot; a miss tail-calls slot one. The generated code
    /// has no entry-kind branch, host-call import or instruction decoder.
    /// The embedding maintains the execution profile. Each destination owns its
    /// entry checks; lookup does not check instruction bytes or CS permissions.
    pub fn compile_dispatcher(&self) -> Result<CompiledModule, BuildError> {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let dispatch = Dispatch::declare(&mut program);
        let transfer = program.function(crate::abi::DISPATCH.signature(), |mut body| {
            let eip = cpu.read_eip(&mut body)?;
            let mut segments = SegmentReads::new(&cpu, self.target.profile);
            let base = segments.base(&mut body, &Segment::Cs.into())?;
            dispatch.lookup(body, &base.add(eip))
        })?;
        crate::abi::DISPATCH.export(&mut program, transfer)?;
        let interpret = program.function(crate::abi::INTERPRET.signature(), |body| {
            dispatch.enter(body, EntryTable::INTERPRETER_SLOT)
        })?;
        crate::abi::INTERPRET.export(&mut program, interpret)?;
        Ok(CompiledModule {
            bytes: program.compile()?,
            entry: crate::abi::DISPATCH.name.into(),
            execution_target: Some(self.target),
        })
    }
}
