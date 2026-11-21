//! Reachable instructions, block boundaries and public entries within one decode view.

use super::{DiscoveryError, DiscoveryLimits};
use crate::Result;
use relocade_x86::{
    BlockError, CodeAddress, CompilerConfig, DecodedBlock, ExecutionMode, InstructionFlow,
    SegmentDefaultSize, SnapshotInstruction, MAX_INSTRUCTION_BYTES,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) struct CodeView {
    origin: CodeAddress,
    size: SegmentDefaultSize,
    config: CompilerConfig,
    instructions: BTreeMap<u32, SnapshotInstruction>,
    boundaries: BTreeSet<u32>,
    exports: BTreeSet<u32>,
}

impl CodeView {
    pub(super) fn new(
        origin: CodeAddress,
        size: SegmentDefaultSize,
        config: CompilerConfig,
    ) -> Self {
        Self {
            origin,
            size,
            config,
            instructions: BTreeMap::new(),
            boundaries: BTreeSet::new(),
            exports: BTreeSet::new(),
        }
    }

    /// Limits apply to instructions added by this root. A join exposes the accepted
    /// instruction as a block entry, even when it was inside a straight-line block.
    pub(super) fn add_root(
        &mut self,
        start: u32,
        mut limits: DiscoveryLimits,
        mut read: impl FnMut(u32, &mut [u8]) -> usize,
    ) -> Result<()> {
        if limits.instructions == 0 || limits.bytes == 0 || limits.block_instructions == 0 {
            return Err(DiscoveryError::ZeroLimit.into());
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
                        eip.wrapping_sub(at) < instruction.next_eip().wrapping_sub(at)
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
                    return Err(DiscoveryError::TooLarge { address: start }.into());
                }
                let instruction = match SnapshotInstruction::decode(bytes, eip, self.size) {
                    Ok(result) => result,
                    Err(error) if decoded.is_empty() => return Err(error.into()),
                    Err(
                        BlockError::TruncatedInstruction { .. }
                        | BlockError::InstructionTooLong { .. }
                        | BlockError::UnsupportedInstruction { .. },
                    ) => break,
                    Err(error) => return Err(error.into()),
                };
                let next = instruction.next_eip();
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
                    return Err(DiscoveryError::TooLarge { address: start }.into());
                }
                let flow = instruction.flow(self.config);
                let ends_block = !matches!(flow, InstructionFlow::Continue);
                match flow {
                    InstructionFlow::Continue | InstructionFlow::Stop => {}
                    InstructionFlow::Fallthrough => enqueue(next, &mut entries, &mut pending),
                    InstructionFlow::Jump { target } => enqueue(target, &mut entries, &mut pending),
                    InstructionFlow::Branch { taken } => {
                        enqueue(taken, &mut entries, &mut pending);
                        enqueue(next, &mut entries, &mut pending);
                    }
                    InstructionFlow::FarCall { .. }
                        if self.config.entry.mode != ExecutionMode::Real => {}
                    InstructionFlow::NearCall { return_eip }
                    | InstructionFlow::FarCall { return_eip } => {
                        // RET dispatches using the offset actually popped from the guest stack.
                        exports.push(return_eip);
                        enqueue(return_eip, &mut entries, &mut pending);
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

    pub(super) fn finish(mut self, blocks: &mut Vec<DecodedBlock>) -> Result<()> {
        let next_in_block = |instruction: &SnapshotInstruction| {
            let next = instruction.next_eip();
            (matches!(instruction.flow(self.config), InstructionFlow::Continue)
                && !self.boundaries.contains(&next))
            .then_some(next)
        };
        for &start in &self.boundaries {
            let Some(first) = self.instructions.remove(&start) else {
                continue;
            };
            let mut next = next_in_block(&first);
            let mut block = DecodedBlock::new(
                self.origin.with_eip(start),
                first,
                self.exports.contains(&start),
            )?;
            while let Some(instruction) = next.and_then(|eip| self.instructions.remove(&eip)) {
                next = next_in_block(&instruction);
                block.push(instruction)?;
            }
            blocks.push(block);
        }
        debug_assert!(self.instructions.is_empty());
        Ok(())
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
