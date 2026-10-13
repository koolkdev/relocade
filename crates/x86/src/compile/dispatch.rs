//! Address resolution shared by every kind of callable execution entry.

mod table;
pub use table::EntryTable;

use wasm86_compiler::{
    BlockBuilder, BuildError, Mem, MemoryImport, Program, Signature, Table, TableImport, Type, Val,
    I32,
};

use crate::{
    memory::Intent, runtime::Runtime, segment::SegmentAccess, state::Cpu, CompiledModule, Segment,
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
    pub fn compile_dispatcher(&self) -> Result<CompiledModule, BuildError> {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let dispatch = Dispatch::declare(&mut program);
        let runtime = Runtime::declare(&mut program, self.execution_budget);
        let function = program.function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |mut body| {
                runtime.check_entry_budget(&mut body, &cpu)?;
                let eip = cpu.read_eip(&mut body)?;
                let check = SegmentAccess::new(&cpu, self.profile).check(
                    &mut body,
                    &Segment::Cs.into(),
                    &eip,
                    1,
                    Intent::Fetch,
                )?;
                if let Some(denied) = check.denied {
                    body.if_(denied, |invalid| {
                        dispatch.enter(invalid, EntryTable::INTERPRETER_SLOT)
                    })?;
                }
                dispatch.lookup(body, &check.linear)
            },
        )?;
        program.export("enter", function)?;
        let transfer = program.function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let observe =
                    body.load_at::<I32>(dispatch.memory, EntryTable::OBSERVE_OFFSET, 0)?;
                body.if_(observe.ne(0), |stopped| stopped.return_(0u64))?;
                body.tail_call(function, &[])
            },
        )?;
        program.export("dispatch", transfer)?;
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
            execution_profile: Some(self.profile),
        })
    }
}
