//! Shared Wasm declarations and function emission for one snapshot batch.

use super::{decode::DecodedBlock, BlockEntry, Compiler};
use crate::{execution::ExecutionModule, BlockError};
use relocade_compiler::Program;

pub(in crate::compile) struct BlockModule<'a> {
    compiler: &'a Compiler,
    pub(in crate::compile) program: Program,
    pub(in crate::compile) entries: Vec<BlockEntry>,
    execution: ExecutionModule,
}

impl<'a> BlockModule<'a> {
    pub(in crate::compile) fn build(
        compiler: &'a Compiler,
        blocks: Vec<DecodedBlock>,
    ) -> Result<Self, BlockError> {
        let mut module = Self::new(compiler);
        module.entries.reserve(blocks.len());
        for block in blocks {
            let entry = module.define_block(block)?;
            module.entries.push(entry);
        }
        Ok(module)
    }

    fn new(compiler: &'a Compiler) -> Self {
        let mut program = Program::new();
        let execution = ExecutionModule::declare(&mut program, compiler.target);
        Self {
            compiler,
            program,
            entries: Vec::new(),
            execution,
        }
    }

    fn define_block(&mut self, block: DecodedBlock) -> Result<BlockEntry, BlockError> {
        let module = self.execution.clone();
        let observed_cpu = self.compiler.observed_cpu;
        let function = self
            .program
            .function::<crate::abi::ExecutionSignature>(move |body, ()| {
                let mut execution = module
                    .builder(body, block.entry.start_eip)?
                    .with_interpreter_fallback()
                    .with_observed_cpu(observed_cpu.as_ref());
                execution.guard_fetch_span(block.entry.byte_len)?;
                for instruction in block.instructions {
                    execution.execute(instruction.bind())?;
                }
                execution.dispatch()
            });
        self.program.export(&block.entry.export, function)?;
        Ok(block.entry)
    }
}
