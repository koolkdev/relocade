//! Compiled guest code and the byte ranges retained by each installation.

use crate::{ConfigurationError, Cpu, DispatchEntry, Error, FarPointer, Result};
use relocade_wasm_runtime::Module;
use relocade_x86::{BlockEntry, CompiledBlocks, ExecutionTarget};
use std::ops::Range;

/// Compiled entries awaiting explicit installation on a compatible, stopped CPU.
/// Clones share the engine module. Each installation creates CPU-local bindings.
#[derive(Clone)]
pub struct CompiledCode {
    pub(super) module: Module,
    pub(super) description: Description,
}

#[derive(Clone)]
pub struct Description {
    pub(super) addresses: Vec<u32>,
    pub(super) ranges: Vec<Range<u64>>,
    pub(super) blocks: Vec<BlockEntry>,
    pub(super) call_targets: Vec<u32>,
    pub(super) target: ExecutionTarget,
}

impl CompiledCode {
    pub(crate) fn new(module: Module, addresses: Vec<u32>, blocks: CompiledBlocks) -> Self {
        Self {
            module,
            description: Description {
                ranges: ranges(&addresses, &blocks.entries),
                addresses,
                blocks: blocks.entries,
                call_targets: blocks.call_targets,
                target: blocks.execution_target,
            },
        }
    }

    /// All decoded guest-byte ranges used by this module, excluding read-ahead
    /// and gaps between blocks. Addresses are linear, before paging and A20.
    /// A u64 end represents the exclusive end of the 32-bit address space.
    pub fn ranges(&self) -> &[Range<u64>] {
        &self.description.ranges
    }

    pub fn blocks(&self) -> &[BlockEntry] {
        &self.description.blocks
    }

    pub(crate) fn call_targets(&self) -> &[u32] {
        &self.description.call_targets
    }

    pub(crate) fn module(&self) -> &Module {
        &self.module
    }

    /// Publishes these entries through the CPU's existing dispatch and hook path.
    /// The caller maintains code/mapping validity for the installed code's lifetime.
    /// Pending or parked host calls must finish before installation.
    pub fn install(&self, cpu: &mut Cpu) -> Result<()> {
        self.install_entries(cpu, false)
    }

    /// Automatic admission preserves entries installed while the batch was pending.
    pub(crate) fn install_missing(&self, cpu: &mut Cpu) -> Result<()> {
        self.install_entries(cpu, true)
    }

    fn entry_key(&self, index: usize) -> Result<u32> {
        let eip = self.description.blocks[index].start_eip;
        let offset = u16::try_from(eip).map_err(|_| ConfigurationError::IncompatibleCpu)?;
        let base = self.description.addresses[index]
            .checked_sub(eip)
            .filter(|base| base & 15 == 0 && *base <= 0xffff0)
            .ok_or(ConfigurationError::IncompatibleCpu)?;
        Ok(FarPointer {
            segment: (base >> 4) as u16,
            offset,
        }
        .packed())
    }

    fn install_entries(&self, cpu: &mut Cpu, missing_only: bool) -> Result<()> {
        if cpu.cpu_state().execution_target() != self.description.target {
            return Err(Error::CompilationTarget);
        }
        let entries: Vec<_> = self
            .description
            .blocks
            .iter()
            .enumerate()
            // Out-of-range continuations retain their internal fetch guards,
            // but have no 16:16 identity to publish in the real-mode map.
            .filter(|(_, block)| block.start_eip <= u32::from(u16::MAX))
            .map(|(index, block)| {
                Ok(DispatchEntry {
                    key: self.entry_key(index)?,
                    export: &block.export,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|entry| !missing_only || !cpu.has_entry(entry.key))
            .collect();
        if entries.is_empty() {
            return Ok(());
        }
        cpu.install_compilation(self, &entries)
    }
}

fn ranges(addresses: &[u32], blocks: &[BlockEntry]) -> Vec<Range<u64>> {
    const END: u64 = 1 << 32;
    let mut ranges = Vec::new();
    for (&address, block) in addresses.iter().zip(blocks) {
        let start = u64::from(address);
        let len = block.byte_len as u64;
        if len >= END {
            return std::iter::once(0..END).collect();
        }
        if len == 0 {
            continue;
        }
        let end = start + len;
        ranges.push(start..end.min(END));
        if end > END {
            ranges.push(0..end - END);
        }
    }
    ranges.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<Range<u64>> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged
            .last_mut()
            .filter(|previous| range.start <= previous.end)
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_merges_neighbors_but_preserves_gaps_and_address_wrap() {
        let blocks: Vec<_> = [4, 3, 2, 4]
            .into_iter()
            .map(|byte_len| BlockEntry {
                start_eip: 0,
                byte_len,
                export: String::new(),
            })
            .collect();
        assert_eq!(
            ranges(&[0x100, 0x104, 0x110, u32::MAX - 1], &blocks),
            [0..2, 0x100..0x107, 0x110..0x112, 0xffff_fffe..0x1_0000_0000]
        );
    }
}
