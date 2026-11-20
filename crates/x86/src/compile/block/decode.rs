//! Snapshot validation and decoding up to each block's execution boundary.

use super::{BlockSnapshot, CodeAddress};
use crate::{
    BlockError, ExecutionTarget, InstructionFlow, SegmentDefaultSize, SnapshotInstruction,
};
use std::collections::HashSet;

/// Contiguous snapshot instructions decoded under one CS.D, with a caller-selected entry.
pub struct DecodedBlock {
    address: CodeAddress,
    code_default_size: SegmentDefaultSize,
    byte_len: usize,
    exported: bool,
    pub(super) instructions: Vec<SnapshotInstruction>,
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
    /// Starts a block with an instruction decoded at `address`. Exported blocks
    /// can be entered through CPU dispatch. Private blocks can receive direct jumps.
    pub fn new(
        address: CodeAddress,
        first: SnapshotInstruction,
        exported: bool,
    ) -> Result<Self, BlockError> {
        let mut block = Self::empty(address, first.code_default_size(), exported);
        block.push(first)?;
        Ok(block)
    }

    pub fn address(&self) -> CodeAddress {
        self.address
    }

    pub fn code_default_size(&self) -> SegmentDefaultSize {
        self.code_default_size
    }

    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    pub fn is_exported(&self) -> bool {
        self.exported
    }

    fn empty(address: CodeAddress, code_default_size: SegmentDefaultSize, exported: bool) -> Self {
        Self {
            address,
            code_default_size,
            byte_len: 0,
            exported,
            instructions: Vec::new(),
        }
    }

    /// Appends a contiguous instruction with the same decode width. Compilation
    /// checks instruction boundaries against the selected execution target.
    pub fn push(&mut self, instruction: SnapshotInstruction) -> Result<(), BlockError> {
        let expected = self.address.eip().wrapping_add(self.byte_len as u32);
        if instruction.eip() != expected {
            return Err(BlockError::NoncontiguousInstruction {
                expected,
                actual: instruction.eip(),
            });
        }
        if instruction.code_default_size() != self.code_default_size {
            return Err(BlockError::InconsistentCodeSize { address: expected });
        }
        self.byte_len += usize::from(instruction.byte_len());
        self.instructions.push(instruction);
        Ok(())
    }

    fn decode(
        snapshot: &BlockSnapshot<'_>,
        target: ExecutionTarget,
        boundaries: &HashSet<CodeAddress>,
    ) -> Result<Self, BlockError> {
        if snapshot.instruction_limit == 0 {
            return Err(BlockError::ZeroInstructionLimit);
        }
        let mut block = Self::empty(snapshot.address, snapshot.code_default_size, true);
        let mut remaining = snapshot.bytes;
        let mut eip = snapshot.address.eip();
        for _ in 0..snapshot.instruction_limit {
            let (decoded, rest) =
                crate::decode::snapshot(remaining, eip, snapshot.code_default_size)?;
            eip = decoded.next_eip();
            remaining = rest;
            let ends_block = decoded.flow(target) != InstructionFlow::Continue;
            block.push(decoded)?;
            if ends_block || boundaries.contains(&snapshot.address.with_eip(eip)) {
                break;
            }
        }
        Ok(block)
    }
}
