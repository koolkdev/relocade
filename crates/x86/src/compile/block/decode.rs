//! Snapshot validation and decoding up to each block's execution boundary.

use super::{BlockEntry, BlockSnapshot};
use crate::{decode::SnapshotInstruction, BlockError, ExecutionTarget};
use std::collections::BTreeSet;

pub(in crate::compile) struct DecodedBlock {
    pub(in crate::compile) entry: BlockEntry,
    pub(in crate::compile) instructions: Vec<SnapshotInstruction>,
}

pub(super) fn blocks(
    snapshots: &[BlockSnapshot<'_>],
    target: ExecutionTarget,
) -> Result<Vec<DecodedBlock>, BlockError> {
    let mut boundaries = BTreeSet::new();
    for snapshot in snapshots {
        if !boundaries.insert(snapshot.start_eip) {
            return Err(BlockError::DuplicateBlock {
                address: snapshot.start_eip,
            });
        }
    }
    snapshots
        .iter()
        .map(|snapshot| DecodedBlock::decode(snapshot, target, &boundaries))
        .collect()
}

impl DecodedBlock {
    pub(in crate::compile) fn new(start_eip: u32) -> Self {
        Self {
            entry: BlockEntry {
                start_eip,
                byte_len: 0,
                export: format!("block_{start_eip:x}"),
            },
            instructions: Vec::new(),
        }
    }

    pub(in crate::compile) fn push(&mut self, instruction: SnapshotInstruction) {
        self.entry.byte_len += instruction.fallthrough_eip.wrapping_sub(instruction.eip) as usize;
        self.instructions.push(instruction);
    }

    fn decode(
        snapshot: &BlockSnapshot<'_>,
        target: ExecutionTarget,
        boundaries: &BTreeSet<u32>,
    ) -> Result<Self, BlockError> {
        if snapshot.instruction_limit == 0 {
            return Err(BlockError::ZeroInstructionLimit);
        }
        let mut block = Self::new(snapshot.start_eip);
        let mut remaining = snapshot.bytes;
        let mut eip = snapshot.start_eip;
        for _ in 0..snapshot.instruction_limit {
            let (decoded, rest) = crate::decode::snapshot(
                remaining,
                eip,
                crate::SegmentDefaultSize::for_profile(target.profile),
            )?;
            eip = decoded.fallthrough_eip;
            remaining = rest;
            let ends_block = decoded.ends_block(target);
            block.push(decoded);
            if ends_block || boundaries.contains(&eip) {
                break;
            }
        }
        Ok(block)
    }
}
