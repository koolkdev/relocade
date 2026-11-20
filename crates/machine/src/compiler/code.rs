//! Compiled guest code and the byte ranges retained by each installation.

use crate::{
    CodeSource, ConfigurationError, Cpu, DispatchEntry, Error, FarPointer, Memory, Result,
};
use relocade_wasm_runtime::Module;
use relocade_x86::{BlockEntry, CodeAddress, CompiledBlocks, ExecutionTarget, SegmentDefaultSize};
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
    pub(super) ranges: Vec<Range<u64>>,
    pub(super) blocks: Vec<BlockEntry>,
    pub(super) call_targets: Vec<u32>,
    pub(super) target: ExecutionTarget,
    source: CodeSource,
}

impl CompiledCode {
    pub(crate) fn new(module: Module, blocks: CompiledBlocks, source: CodeSource) -> Self {
        Self {
            module,
            description: Description {
                ranges: merged_ranges(source.ranges()),
                blocks: blocks.entries,
                call_targets: blocks.call_targets,
                target: blocks.execution_target,
                source,
            },
        }
    }

    /// All decoded guest-byte ranges used by this module, excluding read-ahead
    /// and gaps between blocks. Addresses are linear, before paging and A20.
    /// A u64 end represents the exclusive end of the 32-bit address space.
    pub fn ranges(&self) -> &[Range<u64>] {
        &self.description.ranges
    }

    /// Instruction snapshot used by this compilation.
    pub fn source(&self) -> &CodeSource {
        &self.description.source
    }

    /// CPU dispatch entries exported by this module. Source ranges also cover
    /// the internal blocks reached by direct transfers within the module.
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
    /// Returns [`Error::StaleCompilation`] if the live instruction bytes differ
    /// from the compiled snapshot. If installed bytes change, retire the affected
    /// module before its next execution.
    /// Pending or parked host calls must finish before installation.
    pub fn install(&self, cpu: &mut Cpu) -> Result<()> {
        let entries = self.dispatch_entries(cpu.cpu_state().execution_target(), &cpu.memory())?;
        if entries.is_empty() {
            return Ok(());
        }
        cpu.install_guest_code(self, &entries)
    }

    pub(crate) fn dispatch_entries(
        &self,
        target: ExecutionTarget,
        memory: &Memory,
    ) -> Result<Vec<DispatchEntry<'_>>> {
        if target != self.description.target {
            return Err(Error::CompilationTarget);
        }
        if !self.source().belongs_to(memory) {
            return Err(Error::CompilationMemory);
        }
        let mut entries = Vec::new();
        for block in self.blocks() {
            let CodeAddress::RealMode { segment, eip } = block.address else {
                return Err(ConfigurationError::IncompatibleCpu.into());
            };
            if block.code_default_size != SegmentDefaultSize::Bits16 {
                return Err(ConfigurationError::IncompatibleCpu.into());
            }
            // A discovered continuation beyond IP=FFFF has no real-mode dispatch key.
            let Ok(offset) = u16::try_from(eip) else {
                continue;
            };
            let key = FarPointer { segment, offset }.packed();
            entries.push(DispatchEntry {
                key,
                export: &block.export,
            });
        }
        Ok(entries)
    }
}

fn merged_ranges(ranges: impl Iterator<Item = Range<u64>>) -> Vec<Range<u64>> {
    let mut ranges: Vec<_> = ranges.collect();
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
    fn coverage_merges_adjacent_source_spans_and_preserves_gaps() {
        let spans = [
            0x100..0x104,
            0x104..0x107,
            0x110..0x112,
            0xffff_fffe..0x1_0000_0000,
            0..2,
        ];
        assert_eq!(
            merged_ranges(spans.into_iter()),
            [0..2, 0x100..0x107, 0x110..0x112, 0xffff_fffe..0x1_0000_0000]
        );
    }
}
