//! Chooses blocks and CPU entry points for a batch of guest function requests.

mod walk;

#[cfg(test)]
mod tests;

use crate::Result;
use relocade_x86::{
    CodeAddress, DecodedBlock, ExecutionProfile, ExecutionTarget, InstructionFlow,
    SegmentDefaultSize,
};
use std::{collections::BTreeMap, fmt};

/// Per-root budgets for discovering blocks. A root pays only for new instructions.
/// Joining a previously accepted path does not spend either discovery budget.
#[derive(Clone, Copy, Debug)]
pub struct DiscoveryLimits {
    /// Maximum number of new instructions this root can add.
    pub instructions: u32,
    /// Maximum encoding bytes of new instructions, excluding read-ahead.
    pub bytes: usize,
    /// Maximum instructions per block. Discovery continues in a new block.
    pub block_instructions: u32,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            instructions: 64 * 1024,
            bytes: 1024 * 1024,
            block_instructions: 256,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryError {
    ZeroLimit,
    TooLarge { address: u32 },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLimit => f.write_str("discovery budgets and maximum instructions per block must be nonzero"),
            Self::TooLarge { address } => write!(f, "new instructions from {address:#x} exceed the discovery instruction or byte budget"),
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// A single code source remains stable across roots. Instructions at the same
/// offset share a decode only within the same code segment and default width.
pub(super) struct BlockDiscovery<R> {
    target: ExecutionTarget,
    read: R,
    views: BTreeMap<(Option<u16>, SegmentDefaultSize), walk::CodeView>,
}

impl<R: FnMut(CodeAddress, &mut [u8]) -> usize> BlockDiscovery<R> {
    pub(super) fn new(target: ExecutionTarget, read: R) -> Self {
        Self {
            target,
            read,
            views: BTreeMap::new(),
        }
    }

    pub(super) fn add_root(
        &mut self,
        entry: CodeAddress,
        size: SegmentDefaultSize,
        limits: DiscoveryLimits,
    ) -> Result<()> {
        let view = self
            .views
            .entry((entry.segment(), size))
            .or_insert_with(|| walk::CodeView::new(entry, size, self.target));
        view.add_root(entry.eip(), limits, |eip, bytes| {
            (self.read)(entry.with_eip(eip), bytes)
        })
    }

    pub(super) fn finish(self) -> Result<Vec<DecodedBlock>> {
        let mut blocks = Vec::new();
        for view in self.views.into_values() {
            view.finish(&mut blocks)?;
        }
        Ok(blocks)
    }
}

/// Statically reachable offsets in the same decode view. Callees are separate
/// function requests. A return to the caller can enter at its saved return EIP.
pub(super) fn successors(
    flow: InstructionFlow,
    next: u32,
    target: ExecutionTarget,
) -> [Option<u32>; 2] {
    match flow {
        InstructionFlow::Continue | InstructionFlow::Fallthrough => [Some(next), None],
        InstructionFlow::Jump { target } => [Some(target), None],
        InstructionFlow::Branch { taken } => [Some(taken), Some(next)],
        InstructionFlow::FarCall { .. } if target.profile != ExecutionProfile::Real16 => {
            [None, None]
        }
        InstructionFlow::NearCall { return_eip } | InstructionFlow::FarCall { return_eip } => {
            [Some(return_eip), None]
        }
        InstructionFlow::Stop => [None, None],
    }
}
