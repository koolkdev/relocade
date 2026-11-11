//! Configuration and compilation of snapshot blocks and dispatch.

mod block;
mod dispatch;
mod observations;
pub use observations::{CpuObservations, X87Observations};

pub use block::{
    compile_block_from_bytes, compile_block_from_bytes_with_profile, BlockCompileError, BlockEntry,
    BlockSnapshot, CompiledBlocks,
};
pub use dispatch::EntryTable;

use crate::{CpuModel, CpuState, ExecutionProfile, ExecutionTarget};

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
    target: ExecutionTarget,
    observations: CpuObservations,
}

impl Compiler {
    /// Selects execution assumptions with the default virtual processor model.
    pub fn new(profile: impl Into<ExecutionProfile>) -> Self {
        Self {
            target: ExecutionTarget {
                profile: profile.into(),
                cpu_model: CpuModel::default(),
                instruction_budgets: true,
            },
            observations: CpuObservations::default(),
        }
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
        self.target.cpu_model = cpu_model;
        self
    }

    /// Declares whether this execution target permits instruction budgets.
    /// Disabling them removes budget code from snapshot blocks. The execution
    /// owner must keep the live budget zero for the lifetime of this target.
    /// Interpreters retain runtime checks so their modules remain shared.
    #[must_use]
    pub fn with_instruction_budgets(mut self, enabled: bool) -> Self {
        self.target.instruction_budgets = enabled;
        self
    }

    /// The complete contract shared by every entry compiled by this owner.
    pub fn target(&self) -> ExecutionTarget {
        self.target
    }

    /// Supplies independently optional values for guarded specialization.
    /// Unknown fields stay dynamic; these hints do not constrain entry admission.
    pub fn with_observations(mut self, observations: CpuObservations) -> Self {
        self.observations = observations;
        self
    }

    /// Extracts supported observations for guarded specialization of snapshot blocks.
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
    pub fn specialize_on_cpu(self, cpu: &CpuState) -> Self {
        self.with_observations(CpuObservations::from_cpu(cpu))
    }
}

#[cfg(test)]
mod tests;
