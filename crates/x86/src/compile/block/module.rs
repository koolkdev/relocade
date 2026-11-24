//! Wasm declarations, links and exports for a batch of guest blocks.

use super::{decode::DecodedBlock, BlockCompileError, BlockEntry, CompiledBlocks, Compiler};
use crate::compile::CodeAddress;
use crate::{
    execution::ExecutionModule, instruction::CallTarget, BlockError, ExecutionMode, InstructionFlow,
};
use relocade_compiler::{CompileExecutor, Func, Program};
use std::collections::BTreeMap;

pub(in crate::compile) struct BlockModule<'a> {
    compiler: &'a Compiler,
    program: Program,
    entries: Vec<BlockEntry>,
    spans: Vec<(CodeAddress, usize)>,
    call_targets: Vec<u32>,
    call_slots: BTreeMap<u32, u32>,
    execution: ExecutionModule,
}

impl<'a> BlockModule<'a> {
    pub(in crate::compile) fn build(
        mut self,
        snapshots: &[super::BlockSnapshot<'_>],
    ) -> Result<Self, BlockError> {
        let blocks = self.compiler.decode_blocks(snapshots)?;
        let blocks = self.declare_blocks(blocks);
        self.define_blocks(blocks, self.execution.clone())?;
        Ok(self)
    }

    pub(in crate::compile) fn build_decoded(
        mut self,
        blocks: Vec<DecodedBlock>,
    ) -> Result<Self, BlockError> {
        let blocks = self.declare_blocks(blocks);
        let mut blocks_by_view = BTreeMap::<_, Vec<_>>::new();
        for (block, function) in &blocks {
            let (_, interior) = block
                .instructions
                .split_last()
                .expect("a decoded block has a first instruction");
            if let Some(boundary) = interior.iter().find(|instruction| {
                instruction.flow(self.compiler.config) != InstructionFlow::Continue
            }) {
                return Err(BlockError::InteriorBlockBoundary {
                    address: boundary.eip(),
                });
            }
            blocks_by_view
                .entry((block.address().segment(), block.code_default_size()))
                .or_default()
                .push((block, *function));
        }
        let mut linked_views = BTreeMap::new();
        for ((segment, width), members) in blocks_by_view {
            linked_views.insert((segment, width), self.link_view(segment, &members)?);
        }
        // Links use the segment and decode width. Public entries and source spans
        // follow the caller's order even when blocks from different views alternate.
        self.spans.reserve(blocks.len());
        for (block, function) in blocks {
            let view = (block.address().segment(), block.code_default_size());
            self.define_block(block, function, linked_views[&view].clone())?;
        }
        Ok(self)
    }

    pub(in crate::compile) fn new(compiler: &'a Compiler) -> Self {
        let mut program = Program::new();
        let execution = ExecutionModule::declare(&mut program, compiler.config);
        Self {
            compiler,
            program,
            entries: Vec::new(),
            spans: Vec::new(),
            call_targets: Vec::new(),
            call_slots: BTreeMap::new(),
            execution,
        }
    }

    fn link_view(
        &mut self,
        segment: Option<u16>,
        blocks: &[(&DecodedBlock, Func)],
    ) -> Result<ExecutionModule, BlockError> {
        let mut entries = BTreeMap::new();
        for (block, function) in blocks {
            let address = block.address();
            if entries.insert(address.eip(), *function).is_some() {
                return Err(BlockError::DuplicateBlock { address });
            }
        }
        let mut call_sites = BTreeMap::new();
        if self.compiler.config.entry.mode == Some(ExecutionMode::Real) {
            for instruction in blocks.iter().flat_map(|(block, _)| &block.instructions) {
                let target = match instruction.call_target() {
                    Some(CallTarget::Near(eip)) if eip <= 0xffff => {
                        let Some(segment) = segment else {
                            continue;
                        };
                        (u32::from(segment) << 16) | eip
                    }
                    Some(CallTarget::Far { selector, offset }) if offset <= 0xffff => {
                        (u32::from(selector) << 16) | offset
                    }
                    _ => continue,
                };
                let index = *self.call_slots.entry(target).or_insert_with(|| {
                    let index =
                        u32::try_from(self.call_targets.len()).expect("call table fits u32");
                    self.call_targets.push(target);
                    index
                });
                call_sites.insert(instruction.eip(), index);
            }
        }
        Ok(self.execution.with_blocks(entries, call_sites))
    }

    pub(in crate::compile) fn compile(self) -> Result<CompiledBlocks, BlockError> {
        Ok(CompiledBlocks {
            bytes: self.program.compile()?,
            entries: self.entries,
            spans: self.spans,
            call_targets: self.call_targets,
            config: self.compiler.config,
        })
    }

    pub(in crate::compile) async fn compile_with<E: CompileExecutor>(
        self,
        executor: &E,
    ) -> Result<CompiledBlocks, BlockCompileError<E::Error>> {
        Ok(CompiledBlocks {
            bytes: self.program.compile_with(executor).await?,
            entries: self.entries,
            spans: self.spans,
            call_targets: self.call_targets,
            config: self.compiler.config,
        })
    }

    fn declare_blocks(&mut self, blocks: Vec<DecodedBlock>) -> Vec<(DecodedBlock, Func)> {
        blocks
            .into_iter()
            .map(|block| {
                let function = self.program.declare(crate::abi::RUN.signature());
                (block, function)
            })
            .collect()
    }

    fn define_blocks(
        &mut self,
        blocks: Vec<(DecodedBlock, Func)>,
        module: ExecutionModule,
    ) -> Result<(), BlockError> {
        self.spans.reserve(blocks.len());
        for (block, function) in blocks {
            self.define_block(block, function, module.clone())?;
        }
        Ok(())
    }

    fn define_block(
        &mut self,
        block: DecodedBlock,
        function: Func,
        module: ExecutionModule,
    ) -> Result<(), BlockError> {
        let address = block.address();
        let byte_len = block.byte_len();
        let code_default_size = block.code_default_size();
        let exported = block.is_exported();
        let observed_cpu = self.compiler.observed_cpu;
        self.program.define(function, move |body| {
            let mut execution = module
                .builder(body, address.eip())?
                .with_interpreter_fallback()
                .with_observed_cpu(observed_cpu.as_ref());
            execution.guard_snapshot(code_default_size, byte_len)?;
            for instruction in block.instructions {
                execution.execute(instruction.bind())?;
            }
            execution.dispatch()
        })?;
        self.spans.push((address, byte_len));
        if exported {
            let export = format!("block_{address}");
            self.program
                .export(&export, function)
                .map_err(|error| match error {
                    relocade_compiler::BuildError::DuplicateExport => {
                        BlockError::DuplicateBlock { address }
                    }
                    error => error.into(),
                })?;
            self.entries.push(BlockEntry {
                address,
                code_default_size,
                byte_len,
                export,
            });
        }
        Ok(())
    }
}
