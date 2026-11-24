//! Compiled guest code and the byte ranges retained by each installation.

use crate::{CodeSource, ConfigurationError, Cpu, Error, FarPointer, Memory, Result};
use relocade_wasm_runtime::Module;
use relocade_x86::{BlockEntry, CodeAddress, CompiledBlocks, CompilerConfig, SegmentDefaultSize};
use std::ops::Range;

/// The destination selected for one public guest entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilationBinding<Export> {
    Export { entry: FarPointer, export: Export },
    Interpreter(FarPointer),
}

/// A completed compilation and the bindings to install on a compatible, stopped CPU.
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
    pub(super) config: CompilerConfig,
    source: CodeSource,
    // Export bindings index the block metadata so replies store each export name once.
    bindings: Vec<CompilationBinding<usize>>,
}

impl CompiledCode {
    pub(crate) fn new(
        module: Module,
        blocks: CompiledBlocks,
        source: CodeSource,
        interpreted: Vec<FarPointer>,
    ) -> Result<Self> {
        let mut bindings = Vec::with_capacity(blocks.entries.len() + interpreted.len());
        for (index, block) in blocks.entries.iter().enumerate() {
            let CodeAddress::RealMode { segment, eip } = block.address else {
                return Err(ConfigurationError::IncompatibleCpu.into());
            };
            if block.code_default_size != SegmentDefaultSize::Bits16 {
                return Err(ConfigurationError::IncompatibleCpu.into());
            }
            // A continuation beyond IP=FFFF has no real-mode dispatch key.
            if let Ok(offset) = u16::try_from(eip) {
                bindings.push(CompilationBinding::Export {
                    entry: FarPointer { segment, offset },
                    export: index,
                });
            }
        }
        bindings.extend(interpreted.into_iter().map(CompilationBinding::Interpreter));
        Ok(Self {
            module,
            description: Description {
                ranges: merged_ranges(source.ranges()),
                blocks: blocks.entries,
                call_targets: blocks.call_targets,
                config: blocks.config,
                source,
                bindings,
            },
        })
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

    /// The binding for each addressable public entry, including interpreted blocks.
    pub fn bindings(&self) -> impl ExactSizeIterator<Item = CompilationBinding<&str>> {
        self.description
            .bindings
            .iter()
            .map(|&binding| match binding {
                CompilationBinding::Export { entry, export } => CompilationBinding::Export {
                    entry,
                    export: self.blocks()[export].export.as_str(),
                },
                CompilationBinding::Interpreter(entry) => CompilationBinding::Interpreter(entry),
            })
    }

    /// Publishes these entries through the CPU's existing dispatch and hook path.
    /// Returns [`Error::StaleCompilation`] if the snapshot differs from live memory
    /// or includes a backing byte excluded after it changed in two installations.
    /// Pending or parked host calls must finish before installation.
    pub fn install(&self, cpu: &mut Cpu) -> Result<()> {
        self.check_destination(cpu.cpu_state().compiler_config(), &cpu.memory())?;
        if self.description.bindings.is_empty() {
            return Ok(());
        }
        cpu.install_guest_code(self)
    }

    pub(crate) fn check_destination(&self, config: CompilerConfig, memory: &Memory) -> Result<()> {
        if config != self.description.config {
            return Err(Error::CompilationConfig);
        }
        if !self.source().belongs_to(memory) {
            return Err(Error::CompilationMemory);
        }
        Ok(())
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
