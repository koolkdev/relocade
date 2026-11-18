//! Discover new instructions until an accepted instruction start is reached.

use super::{CodeAddress, DiscoveryLimits};
use crate::{
    compile::block::decode::DecodedBlock,
    decode::{snapshot, SnapshotInstruction},
    instruction::MAX_INSTRUCTION_BYTES,
    BlockError, ExecutionTarget, SegmentDefaultSize,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[cfg(test)]
pub(super) fn blocks(
    entry: CodeAddress,
    size: SegmentDefaultSize,
    target: ExecutionTarget,
    limits: DiscoveryLimits,
    read: impl FnMut(u32, &mut [u8]) -> usize,
) -> Result<Vec<DecodedBlock>, BlockError> {
    let mut code = Discovery::new(entry, size, target);
    code.add_root(entry.eip(), limits, read)?;
    Ok(code.finish())
}

pub(super) struct Discovery {
    origin: CodeAddress,
    size: SegmentDefaultSize,
    target: ExecutionTarget,
    instructions: BTreeMap<u32, SnapshotInstruction>,
    boundaries: BTreeSet<u32>,
    exports: BTreeSet<u32>,
}

impl Discovery {
    pub(super) fn new(
        origin: CodeAddress,
        size: SegmentDefaultSize,
        target: ExecutionTarget,
    ) -> Self {
        Self {
            origin,
            size,
            target,
            instructions: BTreeMap::new(),
            boundaries: BTreeSet::new(),
            exports: BTreeSet::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    /// Limits apply to instructions added by this root. A join exposes the accepted
    /// instruction as a block entry, even when it was inside a straight-line block.
    pub(super) fn add_root(
        &mut self,
        start: u32,
        mut limits: DiscoveryLimits,
        mut read: impl FnMut(u32, &mut [u8]) -> usize,
    ) -> Result<(), BlockError> {
        if limits.instructions == 0 || limits.bytes == 0 || limits.block_instructions == 0 {
            return Err(BlockError::ZeroDiscoveryLimit);
        }
        let mut pending = VecDeque::from([start]);
        let mut entries = BTreeSet::from([start]);
        let mut exports = vec![start];
        // Stage this root so an over-budget path cannot split an accepted block.
        let mut decoded = BTreeMap::<u32, SnapshotInstruction>::new();
        let mut window = Window::default();
        while let Some(mut eip) = pending.pop_front() {
            let mut block_len = 0;
            loop {
                if self.instructions.contains_key(&eip) || decoded.contains_key(&eip) {
                    entries.insert(eip);
                    break;
                }
                // Only encodings on this root's paths preclude an overlapping decode.
                if decoded
                    .range(..=eip)
                    .next_back()
                    .or_else(|| decoded.last_key_value())
                    .is_some_and(|(&at, instruction)| {
                        eip.wrapping_sub(at) < instruction.fallthrough_eip.wrapping_sub(at)
                    })
                {
                    entries.insert(eip);
                    break;
                }
                let bytes = window.at(eip, &mut read);
                if bytes.is_empty() {
                    break;
                }
                if limits.instructions == 0 || limits.bytes == 0 {
                    return Err(BlockError::DiscoveryTooLarge { address: start });
                }
                let (instruction, _) = match snapshot(bytes, eip, self.size) {
                    Ok(result) => result,
                    Err(error) if decoded.is_empty() => return Err(error),
                    Err(
                        BlockError::TruncatedInstruction { .. }
                        | BlockError::InstructionTooLong { .. }
                        | BlockError::UnsupportedInstruction { .. },
                    ) => break,
                    Err(error) => return Err(error),
                };
                let next = instruction.fallthrough_eip;
                let len = next.wrapping_sub(eip) as usize;
                if decoded
                    .range(eip..)
                    .next()
                    .or_else(|| decoded.first_key_value())
                    .is_some_and(|(&at, _)| (at.wrapping_sub(eip) as usize) < len)
                {
                    entries.insert(eip);
                    break;
                }
                if len > limits.bytes {
                    return Err(BlockError::DiscoveryTooLarge { address: start });
                }
                let ends_block = instruction.ends_block(self.target);
                if ends_block {
                    for successor in instruction.successors(self.target).into_iter().flatten() {
                        if instruction.is_call() {
                            exports.push(successor);
                        }
                        enqueue(successor, &mut entries, &mut pending);
                    }
                }
                decoded.insert(eip, instruction);
                limits.instructions -= 1;
                limits.bytes -= len;
                if ends_block {
                    break;
                }
                eip = next;
                block_len += 1;
                if block_len == limits.block_instructions {
                    entries.insert(eip);
                    block_len = 0;
                }
            }
        }
        if !decoded.is_empty() || self.instructions.contains_key(&start) {
            self.exports.extend(exports);
            self.boundaries.extend(entries);
            self.instructions.extend(decoded);
        }
        Ok(())
    }

    pub(super) fn finish(mut self) -> Vec<DecodedBlock> {
        let mut blocks = Vec::new();
        for &start in &self.boundaries {
            let mut block = DecodedBlock::new(
                self.origin.with_eip(start),
                self.size,
                self.exports.contains(&start),
            );
            let mut eip = start;
            while let Some(instruction) = self.instructions.remove(&eip) {
                eip = instruction.fallthrough_eip;
                let ends_block = instruction.ends_block(self.target);
                block.push(instruction);
                if ends_block || self.boundaries.contains(&eip) {
                    break;
                }
            }
            if !block.instructions.is_empty() {
                blocks.push(block);
            }
        }
        debug_assert!(self.instructions.is_empty());
        blocks
    }
}

fn enqueue(eip: u32, entries: &mut BTreeSet<u32>, pending: &mut VecDeque<u32>) {
    if entries.insert(eip) {
        pending.push_back(eip);
    }
}

/// Reuses sequential reads and joins short prefixes, including page boundaries.
/// The decoder sees up to fifteen bytes without retrying a partially decoded instruction.
struct Window {
    start: u32,
    len: usize,
    bytes: [u8; 256],
}

impl Default for Window {
    fn default() -> Self {
        Self {
            start: 0,
            len: 0,
            bytes: [0; 256],
        }
    }
}

impl Window {
    fn at(&mut self, eip: u32, read: &mut impl FnMut(u32, &mut [u8]) -> usize) -> &[u8] {
        let offset = eip.wrapping_sub(self.start) as usize;
        let available = self.len.saturating_sub(offset);
        let needed = MAX_INSTRUCTION_BYTES as usize;
        if available >= needed {
            return &self.bytes[offset..self.len];
        }
        if available != 0 {
            self.bytes.copy_within(offset..self.len, 0);
        }
        self.start = eip;
        self.len = available;
        let capacity = self.bytes.len();
        while self.len < needed {
            let count = read(
                eip.wrapping_add(self.len as u32),
                &mut self.bytes[self.len..capacity],
            );
            assert!(
                count <= capacity - self.len,
                "code reader exceeded its buffer"
            );
            self.len += count;
            if count == 0 {
                break;
            }
        }
        &self.bytes[..self.len]
    }
}
