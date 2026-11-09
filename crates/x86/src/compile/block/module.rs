//! Shared Wasm declarations and function emission for one snapshot batch.

use super::{
    decode::{self, DecodedBlock},
    BlockEntry, BlockSnapshot, CompiledBlocks, Compiler,
};
use crate::{execution::ExecutionModule, BlockError};
use relocade_compiler::Program;

pub(super) struct BlockModule<'a> {
    compiler: &'a Compiler,
    program: Program,
    execution: ExecutionModule,
}

impl<'a> BlockModule<'a> {
    pub(super) fn compile(
        compiler: &'a Compiler,
        snapshots: &[BlockSnapshot<'_>],
    ) -> Result<CompiledBlocks, BlockError> {
        let blocks = decode::blocks(snapshots, compiler.target)?;
        let mut module = Self::new(compiler);
        let mut entries = Vec::with_capacity(blocks.len());
        for block in blocks {
            entries.push(module.define_block(block)?);
        }
        Ok(CompiledBlocks {
            bytes: module.program.compile()?,
            entries,
            execution_target: compiler.target,
        })
    }

    fn new(compiler: &'a Compiler) -> Self {
        let mut program = Program::new();
        let execution = ExecutionModule::declare(&mut program, compiler.target);
        Self {
            compiler,
            program,
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
