//! Shared Wasm declarations and function emission for one snapshot batch.

use super::{
    decode::{self, DecodedBlock},
    BlockEntry, BlockSnapshot, Compiler,
};
use crate::{execution::ExecutionModule, profile::PendingRequirements, BlockError, CompiledBlocks};
use relocade_compiler::{CompileExecutor, Program};

pub(super) struct BlockModule<'a> {
    compiler: &'a Compiler,
    program: Program,
    entries: Vec<(BlockEntry, PendingRequirements)>,
    execution: ExecutionModule,
}

impl<'a> BlockModule<'a> {
    pub(super) fn build(
        compiler: &'a Compiler,
        snapshots: &[BlockSnapshot<'_>],
    ) -> Result<Self, BlockError> {
        let blocks = decode::blocks(snapshots, compiler.target)?;
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

    pub(super) fn compile(self) -> Result<CompiledBlocks, BlockError> {
        let bytes = self.program.compile()?;
        Ok(CompiledBlocks {
            bytes,
            entries: Self::finish_entries(self.entries),
            execution_target: self.compiler.target,
        })
    }

    pub(super) async fn compile_with<E: CompileExecutor>(
        self,
        executor: &E,
    ) -> Result<CompiledBlocks, super::BlockCompileError<E::Error>> {
        let bytes = self.program.compile_with(executor).await?;
        Ok(CompiledBlocks {
            bytes,
            entries: Self::finish_entries(self.entries),
            execution_target: self.compiler.target,
        })
    }

    fn finish_entries(entries: Vec<(BlockEntry, PendingRequirements)>) -> Vec<BlockEntry> {
        entries
            .into_iter()
            .map(|(mut entry, pending)| {
                entry.requirements = pending.finish();
                entry
            })
            .collect()
    }

    fn define_block(
        &mut self,
        block: DecodedBlock,
    ) -> Result<(BlockEntry, PendingRequirements), BlockError> {
        let module = self.execution.clone();
        let observed_cpu = self.compiler.observed_cpu;
        let pending = PendingRequirements::new(block.entry.requirements.clone());
        let body_requirements = pending.clone();
        let function = self
            .program
            .function::<crate::abi::ExecutionSignature>(move |body, ()| {
                body_requirements.build(|facts| {
                    let mut execution = module
                        .builder(body, block.entry.start_eip)?
                        .with_facts(facts)
                        .with_interpreter_fallback()
                        .with_observed_cpu(observed_cpu.as_ref());
                    execution.guard_fetch_span(block.entry.byte_len)?;
                    for instruction in block.instructions {
                        execution.execute(instruction.bind())?;
                    }
                    execution.dispatch()
                })
            });
        self.program.export(&block.entry.export, function)?;
        Ok((block.entry, pending))
    }
}
