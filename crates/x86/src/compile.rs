//! Configuration and compilation of snapshot blocks and dispatch.

mod address;
mod block;
mod config;
mod dispatch;

pub use address::CodeAddress;
pub use block::{
    compile_block_from_bytes, compile_block_from_bytes_with_profile, BlockCompileError, BlockEntry,
    BlockSnapshot, CompiledBlocks, DecodedBlock,
};
pub use config::CompilerConfig;
pub use dispatch::EntryTable;

use crate::{CpuModel, CpuState, ExecutionProfile, MemoryBackend};

/// Compiles snapshot blocks and dispatch for one execution target.
/// CPU observations specialize snapshot blocks. Use [`crate::build_interpreter`]
/// to construct the interpreter program for an execution profile.
///
/// ```
/// use relocade_x86::{Compiler, SegmentProfile};
/// let compiler = Compiler::new(SegmentProfile::Flat32);
/// let block = compiler.compile_block(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
/// assert_eq!(block.entry, "block_1000");
/// # Ok::<(), relocade_x86::BlockError>(())
/// ```
pub struct Compiler {
    config: CompilerConfig,
    observed_cpu: Option<CpuState>,
}

impl Compiler {
    /// Selects the profile guarantees shared by every entry and the default processor model.
    /// Use [`Self::specialize_on_cpu`] for observed state that needs a runtime guard.
    pub fn new(profile: impl Into<ExecutionProfile>) -> Self {
        let profile = profile.into();
        Self::from_config(CompilerConfig::new(
            profile.into(),
            MemoryBackend::default_for(profile),
        ))
    }

    /// Compiles with the supplied entry assumptions, memory routing, CPU model and budgets.
    pub fn from_config(config: CompilerConfig) -> Self {
        Self {
            config,
            observed_cpu: None,
        }
    }

    /// Accepts the execution settings of an existing compiler.
    pub fn from_target(config: CompilerConfig) -> Self {
        Self::from_config(config)
    }

    /// Selects how generated code accesses guest memory after segmentation.
    #[must_use]
    pub fn with_memory_backend(mut self, memory_backend: MemoryBackend) -> Self {
        self.config.memory_backend = memory_backend;
        self
    }

    /// Selects processor capabilities for snapshot blocks and dispatch.
    ///
    /// ```
    /// use relocade_x86::{Compiler, CpuFeature, CpuModel, SegmentProfile};
    /// let model = CpuModel::default().with_feature(CpuFeature::Cmov, false);
    /// let compiler = Compiler::new(SegmentProfile::Flat32).with_cpu_model(model);
    /// let block = compiler.compile_block(0x1000, &[0x0f, 0x44, 0xc0], 1)?;
    /// assert_eq!(block.execution_target, Some(compiler.target()));
    /// // Executing this CMOVE raises #UD without reading its operand.
    /// # Ok::<(), relocade_x86::BlockError>(())
    /// ```
    #[must_use]
    pub fn with_cpu_model(mut self, cpu_model: CpuModel) -> Self {
        self.config.cpu_model = cpu_model;
        self
    }

    /// Declares whether this execution target permits instruction budgets.
    /// Disabling them removes budget code from snapshot blocks. The execution
    /// owner must keep the live budget zero for the lifetime of this target.
    /// Interpreters retain runtime checks so their modules remain shared.
    #[must_use]
    pub fn with_instruction_budgets(mut self, enabled: bool) -> Self {
        self.config.instruction_budgets = enabled;
        self
    }

    /// The execution target shared by every entry. Snapshots also retain their code addresses.
    pub fn config(&self) -> CompilerConfig {
        self.config
    }

    /// Returns the compiler configuration for this artifact.
    pub fn target(&self) -> CompilerConfig {
        self.config()
    }

    /// Copies a CPU snapshot for guarded specialization of snapshot blocks.
    /// Instruction semantics choose which observations to use and check current
    /// values before effects; a mismatch enters the interpreter. Repeated checks
    /// of unchanged values fold away. Interpreter entries do not use observations.
    /// See the [observed-state contract](crate#observed-cpu-state) for consumers.
    ///
    /// ```
    /// use relocade_x86::{Compiler, CpuState, SegmentProfile};
    /// let cpu = CpuState::default();
    /// let compiler = Compiler::new(SegmentProfile::Flat32).specialize_on_cpu(&cpu);
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
