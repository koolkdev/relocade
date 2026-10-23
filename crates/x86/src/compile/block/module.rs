//! Shared Wasm declarations and function emission for one snapshot batch.

use super::{
    decode::{self, DecodedBlock},
    BlockEntry, BlockSnapshot, CompiledBlocks, Compiler,
};
use crate::{
    execution::ExecutionBuilder, memory::Memory, runtime::Runtime, state::Cpu, BlockError,
};
use relocade_compiler::{BlockBuilder, BuildError, Func, Program, Val, I32};
use std::{collections::BTreeMap, rc::Rc};

#[derive(Clone, Copy)]
pub(super) enum Linking {
    Dispatch,
    SameModule,
}

pub(super) struct BlockModule<'a> {
    compiler: &'a Compiler,
    program: Program,
    cpu: Rc<Cpu>,
    memory: Rc<Memory>,
    runtime: Runtime,
    functions: Rc<BTreeMap<u32, Func>>,
    linking: Linking,
}

impl<'a> BlockModule<'a> {
    pub(super) fn compile(
        compiler: &'a Compiler,
        snapshots: &[BlockSnapshot<'_>],
        linking: Linking,
    ) -> Result<CompiledBlocks, BlockError> {
        let blocks = decode::blocks(snapshots, compiler.target)?;
        let mut module = Self::new(compiler, &blocks, linking);
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

    fn new(compiler: &'a Compiler, blocks: &[DecodedBlock], linking: Linking) -> Self {
        let mut program = Program::new();
        let cpu = Rc::new(Cpu::declare(&mut program));
        let memory = Rc::new(Memory::declare(&mut program, compiler.target.profile));
        let runtime = Runtime::declare(&mut program);
        let functions = blocks
            .iter()
            .map(|block| {
                (
                    block.entry.start_eip,
                    program.declare(crate::abi::RUN.signature()),
                )
            })
            .collect();
        Self {
            compiler,
            program,
            cpu,
            memory,
            runtime,
            functions: Rc::new(functions),
            linking,
        }
    }

    fn define_block(&mut self, block: DecodedBlock) -> Result<BlockEntry, BlockError> {
        let function = self.functions[&block.entry.start_eip];
        let cpu = self.cpu.clone();
        let memory = self.memory.clone();
        let functions = self.functions.clone();
        let runtime = self.runtime;
        let target = self.compiler.target;
        let observed_cpu = self.compiler.observed_cpu;
        let linking = self.linking;
        self.program.define(function, move |body| {
            let mut execution =
                ExecutionBuilder::new(body, &cpu, &memory, runtime, block.entry.start_eip, target)?
                    .with_interpreter_fallback()
                    .with_observed_cpu(observed_cpu.as_ref());
            execution.guard_fetch_span(block.entry.byte_len)?;
            for instruction in block.instructions {
                execution.execute(instruction)?;
            }
            match linking {
                Linking::Dispatch => execution.dispatch(),
                Linking::SameModule => {
                    execution.finish_block(|eip| Self::resolve_entry(&functions, eip))
                }
            }
        })?;
        self.program.export(&block.entry.export, function)?;
        Ok(block.entry)
    }

    fn resolve_entry(
        functions: &BTreeMap<u32, Func>,
        eip: &Val<I32>,
    ) -> Result<
        Option<impl for<'body> FnOnce(BlockBuilder<'body>) -> Result<(), BuildError>>,
        BuildError,
    > {
        Ok(eip
            .constant_bits()?
            .and_then(|eip| functions.get(&(eip as u32)).copied())
            .map(|function| move |body: BlockBuilder<'_>| body.tail_call(function, &[])))
    }
}
