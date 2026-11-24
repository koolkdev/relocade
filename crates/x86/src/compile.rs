//! Configuration and compilation of snapshot blocks and dispatch.

mod address;
mod block;
mod config;
mod dispatch;

pub use address::CodeAddress;
pub use block::{
    compile_block_from_bytes, compile_block_from_bytes_with_config, BlockCompileError, BlockEntry,
    BlockSnapshot, CompiledBlocks, DecodedBlock,
};
pub use config::CompilerConfig;
pub use dispatch::EntryTable;

use crate::{CpuModel, CpuState};

/// Compiles snapshot blocks and dispatch with explicit entry assumptions.
/// CPU observations can further specialize snapshot blocks under runtime guards.
///
/// ```
/// use relocade_x86::{Compiler, CompilerConfig, EntryAssumptions, MemoryBackend};
/// let config = CompilerConfig::new(EntryAssumptions::FLAT32, MemoryBackend::HostPageTable);
/// let compiler = Compiler::new(config);
/// let block = compiler.compile_block(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
/// assert_eq!(block.entry, "block_1000");
/// # Ok::<(), relocade_x86::BlockError>(())
/// ```
pub struct Compiler {
    config: CompilerConfig,
    observed_cpu: Option<CpuState>,
}

impl Compiler {
    /// Selects the assumptions established at every entry, memory routing and CPU model.
    /// Use [`Self::specialize_on_cpu`] for observed state that needs a runtime guard.
    pub fn new(config: CompilerConfig) -> Self {
        Self {
            config,
            observed_cpu: None,
        }
    }

    /// Selects processor capabilities for snapshot blocks and dispatch.
    ///
    /// ```
    /// use relocade_x86::{Compiler, CompilerConfig, CpuFeature, CpuModel, EntryAssumptions, MemoryBackend};
    /// let config = CompilerConfig::new(EntryAssumptions::FLAT32, MemoryBackend::HostPageTable);
    /// let model = CpuModel::default().with_feature(CpuFeature::Cmov, false);
    /// let compiler = Compiler::new(config).with_cpu_model(model);
    /// let block = compiler.compile_block(0x1000, &[0x0f, 0x44, 0xc0], 1)?;
    /// assert_eq!(block.config, Some(compiler.config()));
    /// // Executing this CMOVE raises #UD without reading its operand.
    /// # Ok::<(), relocade_x86::BlockError>(())
    /// ```
    #[must_use]
    pub fn with_cpu_model(mut self, cpu_model: CpuModel) -> Self {
        self.config.cpu_model = cpu_model;
        self
    }

    /// Declares whether the execution owner permits instruction budgets.
    /// Disabling them removes budget code from snapshot blocks. The execution
    /// owner must keep the live budget zero whenever running code compiled with this configuration.
    /// Interpreters retain runtime checks so their modules remain shared.
    #[must_use]
    pub fn with_instruction_budgets(mut self, enabled: bool) -> Self {
        self.config.instruction_budgets = enabled;
        self
    }

    /// Returns the build configuration shared by this compiler's entries.
    pub fn config(&self) -> CompilerConfig {
        self.config
    }

    /// Copies a CPU snapshot for guarded specialization of snapshot blocks.
    /// Instruction semantics choose which observations to use and check current
    /// values before effects; a mismatch enters the interpreter. Repeated checks
    /// of unchanged values fold away. Interpreter entries do not use observations.
    /// See the [observed-state contract](crate#observed-cpu-state) for consumers.
    ///
    /// ```
    /// use relocade_x86::{Compiler, CompilerConfig, CpuState, EntryAssumptions, MemoryBackend};
    /// let config = CompilerConfig::new(EntryAssumptions::FLAT32, MemoryBackend::HostPageTable);
    /// let cpu = CpuState::default();
    /// let compiler = Compiler::new(config).specialize_on_cpu(&cpu);
    /// let block = compiler.compile_block(0x1000, &[0xd8, 0xc9], 1)?;
    /// # Ok::<(), relocade_x86::BlockError>(())
    /// ```
    pub fn specialize_on_cpu(mut self, cpu: &CpuState) -> Self {
        self.observed_cpu = Some(*cpu);
        self
    }
}

#[cfg(test)]
mod tests;
