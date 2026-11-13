use super::FunctionLimits;
use crate::{
    compile::block::decode::DecodedBlock,
    decode::{snapshot, SnapshotInstruction},
    instruction::MAX_INSTRUCTION_BYTES,
    BlockError, ExecutionTarget, SegmentDefaultSize,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn blocks(
    start_eip: u32,
    target: ExecutionTarget,
    mut limits: FunctionLimits,
    mut read: impl FnMut(u32, &mut [u8]) -> usize,
) -> Result<Vec<DecodedBlock>, BlockError> {
    if limits.instructions == 0 || limits.bytes == 0 || limits.block_instructions == 0 {
        return Err(BlockError::ZeroFunctionLimit);
    }
    let mut pending = VecDeque::from([start_eip]);
    let mut entries = BTreeSet::from([start_eip]);
    let mut instructions = BTreeMap::<u32, SnapshotInstruction>::new();
    let mut window = Window::default();
    while let Some(mut eip) = pending.pop_front() {
        loop {
            if instructions.contains_key(&eip) {
                enqueue(eip, &mut entries, &mut pending);
                break;
            }
            // The predecessor may cross the 32-bit address wrap.
            if instructions
                .range(..=eip)
                .next_back()
                .or_else(|| instructions.last_key_value())
                .is_some_and(|(&start, instruction)| {
                    eip.wrapping_sub(start) < instruction.fallthrough_eip.wrapping_sub(start)
                })
            {
                break;
            }
            let bytes = window.at(eip, &mut read);
            if bytes.is_empty() {
                break;
            }
            if limits.instructions == 0 || limits.bytes == 0 {
                return Err(BlockError::FunctionTooLarge { address: start_eip });
            }
            let (instruction, _) =
                snapshot(bytes, eip, SegmentDefaultSize::for_profile(target.profile))?;
            let len = instruction.fallthrough_eip.wrapping_sub(eip);
            if instructions
                .range(eip..)
                .next()
                .or_else(|| instructions.first_key_value())
                .is_some_and(|(&next, _)| next.wrapping_sub(eip) < len)
            {
                // This path would overlap an encoding already retained by another path.
                break;
            }
            if len as usize > limits.bytes {
                return Err(BlockError::FunctionTooLarge { address: start_eip });
            }
            let next = instruction.fallthrough_eip;
            let ends_block = instruction.ends_block(target);
            if ends_block {
                for successor in instruction.successors(target).into_iter().flatten() {
                    enqueue(successor, &mut entries, &mut pending);
                }
            }
            instructions.insert(eip, instruction);
            limits.instructions -= 1;
            limits.bytes -= len as usize;
            if ends_block {
                break;
            }
            eip = next;
        }
    }

    // Only now are all admitted branch targets known. Moving the instructions
    // into their final blocks also splits backward targets without decoding again.
    let mut blocks = Vec::new();
    for &entry in &entries {
        let mut block = DecodedBlock::new(entry);
        let mut eip = entry;
        while let Some(instruction) = instructions.remove(&eip) {
            eip = instruction.fallthrough_eip;
            let ends_block = instruction.ends_block(target);
            block.push(instruction);
            if ends_block || entries.contains(&eip) {
                break;
            }
            if block.instructions.len() == limits.block_instructions as usize {
                blocks.push(block);
                block = DecodedBlock::new(eip);
            }
        }
        if !block.instructions.is_empty() {
            blocks.push(block);
        }
    }
    blocks.sort_unstable_by_key(|block| block.entry.start_eip);
    Ok(blocks)
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
