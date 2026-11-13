//! Shared Wasm declarations and function emission for one snapshot batch.

use super::{decode::DecodedBlock, BlockEntry, Compiler};
use crate::{
    execution::ExecutionModule, instruction::CallTarget, BlockError, CallLink, ExecutionProfile,
};
use relocade_compiler::{Func, Program};
use std::collections::BTreeMap;

pub(in crate::compile) struct BlockModule<'a> {
    pub(in crate::compile) compiler: &'a Compiler,
    pub(in crate::compile) program: Program,
    pub(in crate::compile) entries: Vec<BlockEntry>,
    pub(in crate::compile) calls: Vec<CallLink>,
    execution: ExecutionModule,
}

impl<'a> BlockModule<'a> {
    pub(in crate::compile) fn build(
        compiler: &'a Compiler,
        blocks: Vec<DecodedBlock>,
    ) -> Result<Self, BlockError> {
        let mut module = Self::new(compiler);
        let blocks = module.declare_blocks(blocks);
        module.define_blocks(blocks, module.execution.clone())?;
        Ok(module)
    }

    pub(in crate::compile) fn new(compiler: &'a Compiler) -> Self {
        let mut program = Program::new();
        let execution = ExecutionModule::declare(&mut program, compiler.target);
        Self {
            compiler,
            program,
            entries: Vec::new(),
            calls: Vec::new(),
            execution,
        }
    }

    pub(in crate::compile) fn append(
        &mut self,
        blocks: Vec<DecodedBlock>,
    ) -> Result<(), BlockError> {
        let blocks = self.declare_blocks(blocks);
        // Each discovered function has its own CS-relative namespace. Separate
        // functions in this module may use the same EIP under different segments.
        let entries: BTreeMap<_, _> = blocks
            .iter()
            .map(|(block, function)| (block.entry.start_eip, *function))
            .collect();
        let mut call_sites = BTreeMap::new();
        if self.compiler.target.profile == ExecutionProfile::Real16 {
            for instruction in blocks.iter().flat_map(|(block, _)| &block.instructions) {
                let link = match instruction.call_target() {
                    Some(CallTarget::Near(eip)) if eip <= 0xffff && !entries.contains_key(&eip) => {
                        CallLink::Near {
                            caller_entry: self.entries.len(),
                            eip,
                        }
                    }
                    Some(CallTarget::Far { selector, offset }) if offset <= 0xffff => {
                        CallLink::Far {
                            selector,
                            offset: offset as u16,
                        }
                    }
                    _ => continue,
                };
                let index = u32::try_from(self.calls.len()).expect("call table fits u32");
                call_sites.insert(instruction.eip, index);
                self.calls.push(link);
            }
        }
        let module = self.execution.with_function(entries, call_sites);
        self.define_blocks(blocks, module)
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
                .builder(body, block.entry.start_eip)?
                .with_interpreter_fallback()
                .with_observed_cpu(observed_cpu.as_ref());
            execution.guard_fetch_span(block.entry.byte_len)?;
            for instruction in block.instructions {
                execution.execute(instruction.bind())?;
            }
            execution.dispatch()
        })?;
        self.program.export(&block.entry.export, function)?;
        Ok(block.entry)
    }
}
