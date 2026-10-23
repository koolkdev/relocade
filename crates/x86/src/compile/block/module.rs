//! Shared Wasm declarations and function emission for one snapshot batch.

use super::{
    decode::{self, DecodedBlock},
    BlockEntry, BlockSnapshot, CompiledBlocks, Compiler,
};
use crate::{
    execution::ExecutionBuilder, memory::Memory, runtime::Runtime, state::Cpu, BlockError,
};
use relocade_compiler::Program;
use std::rc::Rc;

pub(super) struct BlockModule<'a> {
    compiler: &'a Compiler,
    program: Program,
    cpu: Rc<Cpu>,
    memory: Rc<Memory>,
    runtime: Runtime,
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
        let cpu = Rc::new(Cpu::declare(&mut program));
        let memory = Rc::new(Memory::declare(&mut program, compiler.target.profile));
        let runtime = Runtime::declare(&mut program);
        Self {
            compiler,
            program,
            cpu,
            memory,
            runtime,
        }
    }

    fn define_block(&mut self, block: DecodedBlock) -> Result<BlockEntry, BlockError> {
        let cpu = self.cpu.clone();
        let memory = self.memory.clone();
        let runtime = self.runtime;
        let target = self.compiler.target;
        let observed_cpu = self.compiler.observed_cpu;
        let function = self
            .program
            .function::<crate::abi::ExecutionSignature>(move |body, ()| {
                let mut execution = ExecutionBuilder::new(
                    body,
                    &cpu,
                    &memory,
                    runtime,
                    block.entry.start_eip,
                    target,
                )?
                .with_interpreter_fallback()
                .with_observed_cpu(observed_cpu.as_ref());
                execution.guard_fetch_span(block.entry.byte_len)?;
                for instruction in block.instructions {
                    execution.execute(instruction)?;
                }
                execution.dispatch()
            });
        self.program.export(&block.entry.export, function)?;
        Ok(block.entry)
    }
}
