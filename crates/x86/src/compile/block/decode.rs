//! Snapshot validation and decoding up to each block's execution boundary.

use super::{BlockSnapshot, CodeAddress};
use crate::{decode::SnapshotInstruction, BlockError, ExecutionTarget, SegmentDefaultSize};
use std::collections::HashSet;

pub(in crate::compile) struct DecodedBlock {
    pub(in crate::compile) address: CodeAddress,
    pub(in crate::compile) code_default_size: SegmentDefaultSize,
    pub(in crate::compile) byte_len: usize,
    pub(in crate::compile) exported: bool,
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
        exported: bool,
    ) -> Self {
        Self {
            address,
            code_default_size,
            byte_len: 0,
            exported,
            instructions: Vec::new(),
        }
    }

    pub(in crate::compile) fn push(&mut self, instruction: SnapshotInstruction) {
        self.byte_len += instruction.fallthrough_eip.wrapping_sub(instruction.eip) as usize;
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
        let mut block = Self::new(snapshot.address, snapshot.code_default_size, true);
        let mut remaining = snapshot.bytes;
        let mut eip = snapshot.address.eip();
        for _ in 0..snapshot.instruction_limit {
            let (decoded, rest) =
                crate::decode::snapshot(remaining, eip, snapshot.code_default_size)?;
            eip = decoded.fallthrough_eip;
            remaining = rest;
            let ends_block = decoded.ends_block(target);
            block.push(decoded);
            if ends_block || boundaries.contains(&snapshot.address.with_eip(eip)) {
                break;
            }
        }
        Ok(block)
    }
}
