//! Snapshot validation and decoding up to each block's execution boundary.

use super::{BlockEntry, BlockSnapshot};
use crate::{instruction::DecodedInstruction, BlockError, ExecutionTarget};
use std::collections::BTreeSet;

pub(super) struct DecodedBlock {
    pub(super) entry: BlockEntry,
    pub(super) instructions: Vec<DecodedInstruction<u32, u32>>,
}

pub(super) fn blocks(
    snapshots: &[BlockSnapshot<'_>],
    target: ExecutionTarget,
    registered_boundaries: &BTreeSet<u32>,
) -> Result<Vec<DecodedBlock>, BlockError> {
    let mut boundaries = BTreeSet::new();
    for snapshot in snapshots {
        if !boundaries.insert(snapshot.start_eip) {
            return Err(BlockError::DuplicateBlock {
                address: snapshot.start_eip,
            });
        }
    }
    boundaries.extend(registered_boundaries);
    snapshots
        .iter()
        .map(|snapshot| DecodedBlock::decode(snapshot, target, &boundaries))
        .collect()
}

impl DecodedBlock {
    fn decode(
        snapshot: &BlockSnapshot<'_>,
        target: ExecutionTarget,
        boundaries: &BTreeSet<u32>,
    ) -> Result<Self, BlockError> {
        if snapshot.instruction_limit == 0 {
            return Err(BlockError::ZeroInstructionLimit);
        }
        let mut instructions = Vec::new();
        let mut remaining = snapshot.bytes;
        let mut eip = snapshot.start_eip;
        for _ in 0..snapshot.instruction_limit {
            let (decoded, rest) =
                crate::decode::snapshot(remaining, eip, target.profile.code_default_size())?;
            eip = decoded.fallthrough_eip;
            remaining = rest;
            let ends_block =
                decoded.instruction.ends_block() || decoded.instruction.fails_requirements(target);
            instructions.push(decoded);
            if ends_block || boundaries.contains(&eip) {
                break;
            }
        }
        Ok(Self {
            entry: BlockEntry {
                start_eip: snapshot.start_eip,
                byte_len: snapshot.bytes.len() - remaining.len(),
                export: format!("block_{:x}", snapshot.start_eip),
                successors: Vec::new(),
            },
            instructions,
        })
    }
}
