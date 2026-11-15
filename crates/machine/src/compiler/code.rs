//! Compiled guest code and the byte ranges retained by each installation.

use crate::{ConfigurationError, Cpu, DispatchEntry, Error, FarPointer, Result};
use relocade_wasm_runtime::Module;
use relocade_x86::{BlockEntry, CompiledBlocks, ExecutionTarget, StateAssumption};
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
    pub(super) state_assumptions: Vec<StateAssumption>,
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
                state_assumptions: blocks.state_assumptions,
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

    pub fn state_assumptions(&self) -> &[StateAssumption] {
        &self.description.state_assumptions
    }

    pub(crate) fn call_targets(&self) -> &[u32] {
        &self.description.call_targets
    }

    pub(crate) fn module(&self) -> &Module {
        &self.module
    }

    /// Publishes these entries through the CPU's existing dispatch and hook path.
    /// State requirements are checked before publishing any entry. The caller
    /// maintains code/mapping validity for the installed code's lifetime.
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
        let state = cpu.cpu_state();
        let snapshot = state.snapshot();
        let target_matches = self
            .description
            .target
            .is_compatible_with(state.execution_target(), &snapshot);
        let assumptions_match = self
            .state_assumptions()
            .iter()
            .all(|requirement| requirement.matches(&snapshot));
        if !target_matches || !assumptions_match {
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

    async fn entry_admission() {
        use crate::{compiler::Reply, worker::Transfer, CpuConfig, MachineConfig, Memory};
        use relocade_wasm_runtime::{WasmBackend, WasmCompiler, WasmRuntime};
        use relocade_x86::{
            BlockSnapshot, Compiler, CpuState, ExecutionProfile, PhysicalMapping,
            PhysicalMemoryMap, SegmentProfile,
        };

        let compiler = Compiler::new(ExecutionProfile::Real16);
        let blocks = compiler
            .compile_blocks(&[BlockSnapshot {
                start_eip: 0x100,
                bytes: &[0x90],
                instruction_limit: 1,
            }])
            .unwrap();
        let module = WasmRuntime::new()
            .unwrap()
            .compiler()
            .compile(&blocks.bytes)
            .await
            .unwrap();
        let ordinary = CompiledCode::new(module, vec![0x100], blocks);
        let mut restricted = ordinary.clone();
        // An extra requirement must survive transfer and participate in admission
        // even when the target itself agrees with the receiving CPU.
        let protected = Compiler::new(SegmentProfile::Flat32)
            .target()
            .state_assumptions()
            .into_iter()
            .find(|requirement| !requirement.matches(&CpuState::real_mode()))
            .unwrap();
        restricted.description.state_assumptions.push(protected);
        let expected = restricted.state_assumptions().to_vec();
        let (state, payload) = Reply(Ok(restricted)).encode();
        let restricted = Reply::decode(state, payload).0.unwrap();
        assert_eq!(restricted.state_assumptions(), expected);

        let map = PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
            .unwrap();
        let memory = Memory::new(MachineConfig::new(0x1000, map).unwrap()).unwrap();
        memory.view().write(0x100, &[0x90]).unwrap();
        let config = CpuConfig::new(compiler.target(), CpuState::real_mode()).unwrap();
        let mut cpu = Cpu::new(memory, config, Default::default()).await.unwrap();
        assert!(matches!(
            restricted.install(&mut cpu),
            Err(Error::CompilationTarget)
        ));
        assert!(matches!(
            restricted.install_missing(&mut cpu),
            Err(Error::CompilationTarget)
        ));
        assert!(!cpu.has_entry(0x100));
        ordinary.install(&mut cpu).unwrap();
        assert!(cpu.has_entry(0x100));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn transferred_requirements_are_checked_before_publishing_entries() {
        pollster::block_on(entry_admission());
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn transferred_requirements_are_checked_before_publishing_entries() {
        entry_admission().await;
    }

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
