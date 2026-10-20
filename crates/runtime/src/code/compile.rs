//! Connected-block planning and lowering from an immutable code image.

use super::BlockRequest;
use crate::Result;
use relocade_x86::{BlockError, BlockSnapshot, CompiledBlocks, Compiler, ExecutionTarget};
use std::collections::{BTreeSet, VecDeque};

#[cfg_attr(
    feature = "compiler-wire",
    derive(serde::Serialize, serde::Deserialize)
)]
pub(super) struct Region {
    pub start_eip: u32,
    pub bytes: Vec<u8>,
}

#[cfg_attr(
    feature = "compiler-wire",
    derive(serde::Serialize, serde::Deserialize)
)]
pub(super) struct CompilationInput {
    pub target: ExecutionTarget,
    pub base: u32,
    pub boundaries: BTreeSet<u32>,
    pub fixed: BTreeSet<u32>,
    pub regions: Vec<Region>,
    pub requests: Vec<BlockRequest>,
    /// Zero for explicit entries; otherwise discover at most this many blocks.
    pub max_blocks: usize,
}

#[cfg_attr(
    feature = "compiler-wire",
    derive(serde::Serialize, serde::Deserialize)
)]
pub(super) struct CompiledCode {
    pub base: u32,
    pub module: CompiledBlocks,
    pub snapshots: Vec<Vec<u8>>,
}

impl CompilationInput {
    pub fn append(&mut self, start_eip: u32, bytes: Vec<u8>) {
        if let Some(last) = self.regions.last_mut() {
            if last.start_eip + last.bytes.len() as u32 == start_eip {
                last.bytes.extend(bytes);
                return;
            }
        }
        self.regions.push(Region { start_eip, bytes });
    }

    pub fn can_start(&self, eip: u32) -> bool {
        !self.fixed.contains(&eip)
            && self.regions.iter().any(|region| {
                eip >= region.start_eip && (eip - region.start_eip) < region.bytes.len() as u32
            })
    }

    fn block(&self, request: BlockRequest) -> Option<BlockSnapshot<'_>> {
        if !self.can_start(request.start_eip) {
            return None;
        }
        let region = self.regions.iter().find(|region| {
            request.start_eip >= region.start_eip
                && (request.start_eip - region.start_eip) < region.bytes.len() as u32
        })?;
        let offset = (request.start_eip - region.start_eip) as usize;
        let remaining = &region.bytes[offset..];
        let mut len =
            (remaining.len() as u64).min(u64::from(request.instruction_limit) * 15) as usize;
        if let Some(boundary) = self.boundaries.range((request.start_eip + 1)..).next() {
            len = len.min((boundary - request.start_eip) as usize);
        }
        Some(BlockSnapshot {
            start_eip: request.start_eip,
            bytes: &remaining[..len],
            instruction_limit: request.instruction_limit,
        })
    }

    pub fn compile(&self) -> Result<Option<CompiledCode>> {
        let compiler = Compiler::new(self.target.profile)
            .with_cpu_model(self.target.cpu_model)
            .with_code_base(self.base)
            .with_block_boundaries(self.boundaries.iter().copied());
        let mut pending: VecDeque<_> = self.requests.iter().copied().collect();
        let mut inputs = Vec::new();
        let mut seen = BTreeSet::new();
        while let Some(request) = pending.pop_front() {
            if self.max_blocks != 0 && !seen.insert(request.start_eip) {
                continue;
            }
            let Some(block) = self.block(request) else {
                if self.max_blocks == 0 {
                    return Ok(None);
                }
                continue;
            };
            if self.max_blocks != 0 {
                match compiler.analyze_blocks(&[block]) {
                    Ok(entries) => pending.extend(entries[0].successors.iter().map(|&start_eip| {
                        BlockRequest {
                            start_eip,
                            ..request
                        }
                    })),
                    Err(BlockError::Compiler(error)) => return Err(error.into()),
                    Err(_) => continue,
                }
            }
            inputs.push(block);
            if self.max_blocks != 0 && inputs.len() == self.max_blocks {
                break;
            }
        }
        if inputs.is_empty() {
            return Ok(None);
        }
        let module = match compiler.compile_blocks(&inputs) {
            Ok(module) => module,
            Err(BlockError::Compiler(error)) => return Err(error.into()),
            Err(_) => return Ok(None),
        };
        let snapshots = inputs
            .iter()
            .zip(&module.entries)
            .map(|(block, entry)| block.bytes[..entry.byte_len].to_vec())
            .collect();
        Ok(Some(CompiledCode {
            base: self.base,
            module,
            snapshots,
        }))
    }
}
