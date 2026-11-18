//! Snapshot validation and decoding up to each block's execution boundary.

use super::{BlockEntry, BlockSnapshot, CodeAddress};
use crate::{
    decode::SnapshotInstruction, BlockError, ExecutionTarget, InstructionFlow, SegmentDefaultSize,
};
use std::collections::HashSet;

pub(in crate::compile) struct DecodedBlock {
    pub(in crate::compile) entry: BlockEntry,
    pub(in crate::compile) instructions: Vec<SnapshotInstruction>,
}

pub(super) fn blocks(
    snapshots: &[BlockSnapshot<'_>],
    target: ExecutionTarget,
) -> Result<Vec<DecodedBlock>, BlockError> {
    let mut boundaries = HashSet::new();
    for snapshot in snapshots {
        if !boundaries.insert(snapshot.address) {
            return Err(BlockError::DuplicateBlock {
                address: snapshot.address,
            });
        }
    }
    snapshots
        .iter()
        .map(|snapshot| DecodedBlock::decode(snapshot, target, &boundaries))
        .collect()
}

impl DecodedBlock {
    pub(in crate::compile) fn new(
        address: CodeAddress,
        code_default_size: SegmentDefaultSize,
    ) -> Self {
        Self {
            entry: BlockEntry {
                address,
                code_default_size,
                byte_len: 0,
                export: format!("block_{address}"),
            },
            instructions: Vec::new(),
        }
    }

    pub(in crate::compile) fn push(&mut self, instruction: SnapshotInstruction) {
        self.entry.byte_len += instruction.next_eip().wrapping_sub(instruction.eip()) as usize;
        self.instructions.push(instruction);
    }

    fn decode(
        snapshot: &BlockSnapshot<'_>,
        target: ExecutionTarget,
        boundaries: &HashSet<CodeAddress>,
    ) -> Result<Self, BlockError> {
        if snapshot.instruction_limit == 0 {
            return Err(BlockError::ZeroInstructionLimit);
        }
        let mut block = Self::new(snapshot.address, snapshot.code_default_size);
        let mut remaining = snapshot.bytes;
        let mut eip = snapshot.address.eip();
        for _ in 0..snapshot.instruction_limit {
            let (decoded, rest) =
                crate::decode::snapshot(remaining, eip, snapshot.code_default_size)?;
            eip = decoded.next_eip();
            remaining = rest;
            let ends_block = decoded.flow(target) != InstructionFlow::Continue;
            block.push(decoded);
            if ends_block || boundaries.contains(&snapshot.address.with_eip(eip)) {
                break;
            }
        }
        Ok(block)
    }
}
