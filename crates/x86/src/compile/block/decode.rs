//! Snapshot validation and decoding up to each block's execution boundary.

use super::{BlockEntry, BlockSnapshot, CodeAddress};
use crate::{decode::SnapshotInstruction, BlockError, ExecutionTarget};
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
        if !boundaries.insert(snapshot.address()) {
            return Err(BlockError::DuplicateBlock {
                address: snapshot.start_eip,
            });
        }
    }
    snapshots
        .iter()
        .map(|snapshot| {
            DecodedBlock::new(snapshot.address()).decode(
                snapshot.bytes,
                snapshot.instruction_limit,
                target,
                &boundaries,
            )
        })
        .collect()
}

impl DecodedBlock {
    pub(in crate::compile) fn new(address: CodeAddress) -> Self {
        Self {
            entry: BlockEntry {
                address,
                byte_len: 0,
                export: format!("block_{:x}", address.eip()),
            },
            instructions: Vec::new(),
        }
    }

    pub(in crate::compile) fn push(&mut self, instruction: SnapshotInstruction) {
        self.entry.byte_len += instruction.fallthrough_eip.wrapping_sub(instruction.eip) as usize;
        self.instructions.push(instruction);
    }

    pub(super) fn decode(
        mut self,
        mut remaining: &[u8],
        instruction_limit: u32,
        target: ExecutionTarget,
        boundaries: &HashSet<CodeAddress>,
    ) -> Result<Self, BlockError> {
        if instruction_limit == 0 {
            return Err(BlockError::ZeroInstructionLimit);
        }
        let address = self.entry.address;
        let mut eip = address.eip();
        let code_default_size = address.code_default(target.profile);
        for _ in 0..instruction_limit {
            let (decoded, rest) = crate::decode::snapshot(remaining, eip, code_default_size)?;
            eip = decoded.fallthrough_eip;
            remaining = rest;
            let ends_block = decoded.ends_block(target);
            self.push(decoded);
            if ends_block || boundaries.contains(&address.with_eip(eip)) {
                break;
            }
        }
        Ok(self)
    }
}
