//! Address resolution shared by every kind of callable execution entry.

mod table;
pub use table::EntryTable;

use wasm86_compiler::{
    BlockBuilder, BuildError, Mem, MemoryImport, Program, Signature, Table, TableImport, Type, Val,
    I32,
};

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
            memory: program.import_memory(MemoryImport {
                module: "wasm86".into(),
                name: "executionEntries".into(),
                minimum: 1,
                maximum: None,
                shared: false,
            }),
            functions: program.import_table(TableImport {
                module: "wasm86".into(),
                name: "executionFunctions".into(),
                minimum: 2,
                maximum: None,
                signature: Signature {
                    parameters: vec![],
                    results: vec![Type::I64],
                },
            }),
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
        let transfer = program.function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let eip = body.parameter::<I32>(0)?;
                let mut segments = SegmentReads::new(&cpu, self.target.profile);
                let base = segments.base(&mut body, &Segment::Cs.into())?;
                dispatch.lookup(body, &base.add(eip))
            },
        )?;
        program.export("dispatch", transfer)?;
        let enter = program.function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |mut body| {
                let eip = cpu.read_eip(&mut body)?;
                body.tail_call(transfer, &[eip.into()])
            },
        )?;
        program.export("enter", enter)?;
        let interpret = program.function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |body| dispatch.enter(body, EntryTable::INTERPRETER_SLOT),
        )?;
        program.export("interpret", interpret)?;
        Ok(CompiledModule {
            bytes: program.compile()?,
            entry: "enter".into(),
            execution_target: Some(self.target),
        })
    }
}
