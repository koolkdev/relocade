//! Shared Wasm declarations and function emission for one snapshot batch.

use super::{decode::DecodedBlock, BlockCompileError, BlockEntry, CompiledBlocks, Compiler};
use crate::compile::CodeAddress;
use crate::{
    execution::ExecutionModule, instruction::CallTarget, profile::EntryGuarantees, BlockError,
    ExecutionProfile,
};
use relocade_compiler::{CompileExecutor, Func, Program};
use std::collections::BTreeMap;

pub(in crate::compile) struct BlockModule<'a> {
    pub(in crate::compile) compiler: &'a Compiler,
    pub(in crate::compile) program: Program,
    pub(in crate::compile) entries: Vec<BlockEntry>,
    pub(in crate::compile) call_targets: Vec<u32>,
    call_slots: BTreeMap<u32, u32>,
    execution: ExecutionModule,
}

impl<'a> BlockModule<'a> {
    pub(in crate::compile) fn build(
        mut self,
        snapshots: &[super::BlockSnapshot<'_>],
    ) -> Result<Self, BlockError> {
        let blocks = super::decode::blocks(snapshots, self.compiler.target)?;
        let blocks = self.declare_blocks(blocks);
        self.define_blocks(blocks, self.execution.clone())?;
        Ok(self)
    }

    pub(in crate::compile) fn new(compiler: &'a Compiler) -> Self {
        Self::with_entry_guarantees(
            compiler,
            EntryGuarantees::for_profile(compiler.target.profile),
        )
    }

    pub(in crate::compile) fn with_entry_guarantees(
        compiler: &'a Compiler,
        guarantees: EntryGuarantees,
    ) -> Self {
        let mut program = Program::new();
        let execution = ExecutionModule::declare(&mut program, compiler.target, guarantees);
        Self {
            compiler,
            program,
            entries: Vec::new(),
            call_targets: Vec::new(),
            call_slots: BTreeMap::new(),
            execution,
        }
    }

    pub(in crate::compile) fn append(
        &mut self,
        blocks: Vec<DecodedBlock>,
        entry: CodeAddress,
    ) -> Result<(), BlockError> {
        let blocks = self.declare_blocks(blocks);
        // Each discovered function has its own CS-relative namespace. Separate
        // functions in this module may use the same EIP under different segments.
        let entries: BTreeMap<_, _> = blocks
            .iter()
            .map(|(block, function)| (block.entry.address.eip(), *function))
            .collect();
        let mut call_sites = BTreeMap::new();
        if self.compiler.target.profile == ExecutionProfile::Real16 {
            for instruction in blocks.iter().flat_map(|(block, _)| &block.instructions) {
                let target = match instruction.call_target() {
                    Some(CallTarget::Near(eip)) if eip <= 0xffff && !entries.contains_key(&eip) => {
                        let CodeAddress::RealMode { segment, .. } = entry else {
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
        let module = self.execution.with_function(entries, call_sites);
        self.define_blocks(blocks, module)
    }

    pub(in crate::compile) fn compile(self) -> Result<CompiledBlocks, BlockError> {
        Ok(CompiledBlocks {
            bytes: self.program.compile()?,
            entries: self.entries,
            call_targets: self.call_targets,
            execution_target: self.compiler.target,
        })
    }

    pub(in crate::compile) async fn compile_with<E: CompileExecutor>(
        self,
        executor: &E,
    ) -> Result<CompiledBlocks, BlockCompileError<E::Error>> {
        Ok(CompiledBlocks {
            bytes: self.program.compile_with(executor).await?,
            entries: self.entries,
            call_targets: self.call_targets,
            execution_target: self.compiler.target,
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
        self.entries.reserve(blocks.len());
        for (block, function) in blocks {
            let entry = self.define_block(block, function, module.clone())?;
            self.entries.push(entry);
        }
        Ok(())
    }

    fn define_block(
        &mut self,
        block: DecodedBlock,
        function: Func,
        module: ExecutionModule,
    ) -> Result<BlockEntry, BlockError> {
        let observed_cpu = self.compiler.observed_cpu;
        self.program.define(function, move |body| {
            let mut execution = module
                .builder(body, block.entry.address.eip())?
                .with_interpreter_fallback()
                .with_observed_cpu(observed_cpu.as_ref());
            execution.guard_snapshot(block.entry.code_default_size, block.entry.byte_len)?;
            for instruction in block.instructions {
                execution.execute(instruction.bind())?;
            }
            execution.dispatch()
        })?;
        self.program.export(&block.entry.export, function)?;
        Ok(block.entry)
    }
}
