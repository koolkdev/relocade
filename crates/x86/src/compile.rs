//! Configuration and construction of snapshot and runtime-decoded execution entries.

mod block;
mod dispatch;
mod host;
mod interpreter;

pub use block::{compile_block_from_bytes, compile_block_from_bytes_with_profile};
pub use dispatch::EntryTable;
pub use host::{CallableEntry, CompiledHostFunctions, HostFunction, HostReturn};
pub use interpreter::{compile_interpreter, compile_interpreter_step};

use crate::{CpuModel, CpuState, ExecutionProfile, ExecutionTarget};

/// Generates snapshot blocks and interpreter entries for one execution target.
/// CPU observations specialize snapshot blocks; interpreters read live state.
///
/// ```
/// use wasm86_x86::{Compiler, SegmentProfile};
/// let compiler = Compiler::new(SegmentProfile::Flat32);
/// let block = compiler.compile_block(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
/// let run = compiler.compile_interpreter()?;
/// let step = compiler.compile_interpreter_step()?;
/// assert_eq!(run.entry, "run");
/// assert_eq!(step.entry, "step");
/// # Ok::<(), wasm86_x86::BlockError>(())
/// ```
pub struct Compiler {
    target: ExecutionTarget,
    observed_cpu: Option<CpuState>,
    execution_budget: bool,
}

impl Compiler {
    /// Selects execution assumptions with the default virtual processor model.
    pub fn new(profile: impl Into<ExecutionProfile>) -> Self {
        Self {
            target: ExecutionTarget {
                profile: profile.into(),
                cpu_model: CpuModel::default(),
            },
            observed_cpu: None,
            execution_budget: false,
        }
    }

    /// Selects the same processor capabilities for blocks and interpreter entries.
    ///
    /// ```
    /// use wasm86_x86::{Compiler, CpuFeature, CpuModel, SegmentProfile};
    /// let model = CpuModel::default().with_feature(CpuFeature::Cmov, false);
    /// let compiler = Compiler::new(SegmentProfile::Flat32).with_cpu_model(model);
    /// let block = compiler.compile_block(0x1000, &[0x0f, 0x44, 0xc0], 1)?;
    /// assert_eq!(block.execution_target, Some(compiler.target()));
    /// // Executing this CMOVE raises #UD without reading its operand.
    /// # Ok::<(), wasm86_x86::BlockError>(())
    /// ```
    #[must_use]
    pub fn with_cpu_model(mut self, cpu_model: CpuModel) -> Self {
        self.target.cpu_model = cpu_model;
        self
    }

    /// Processor capabilities and execution assumptions shared by these entries.
    /// Cache/link keys must also include [`Self::has_execution_budget`].
    pub fn target(&self) -> ExecutionTarget {
        self.target
    }

    /// Scheduling policy required by every entry compiled by this owner.
    pub fn has_execution_budget(&self) -> bool {
        self.execution_budget
    }

    /// Enables cooperative scheduling through `wasm86.executionBudget`.
    /// Ordinary instructions are charged together at block exits; REP uses
    /// bounded chunks. Slices can overshoot by at most
    /// [`crate::MAX_EXECUTION_CHUNK`] minus one work units. All entries sharing
    /// a CPU must use this policy for the slice contract to hold.
    /// See the [slice contract](crate#execution-slices).
    pub fn with_execution_budget(mut self) -> Self {
        self.execution_budget = true;
        self
    }

    /// Copies a CPU snapshot for guarded specialization of snapshot blocks.
    /// Instruction semantics choose which observations to use and check current
    /// values before effects; a mismatch enters the interpreter. Repeated checks
    /// of unchanged values fold away. Interpreter entries do not use observations.
    /// See the [observed-state contract](crate#observed-cpu-state) for consumers.
    ///
    /// ```
    /// use wasm86_x86::{Compiler, CpuState, SegmentProfile};
    /// let cpu = CpuState::default();
    /// let compiler = Compiler::new(SegmentProfile::Flat32).specialize_on_cpu(&cpu);
    /// let block = compiler.compile_block(0x1000, &[0xd8, 0xc9], 1)?;
    /// # Ok::<(), wasm86_x86::BlockError>(())
    /// ```
    pub fn specialize_on_cpu(mut self, cpu: &CpuState) -> Self {
        self.observed_cpu = Some(*cpu);
        self
    }
}

#[cfg(test)]
mod tests;
