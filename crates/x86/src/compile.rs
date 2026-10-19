//! Configuration and construction of snapshot and runtime-decoded execution entries.

mod block;
mod dispatch;
mod interpreter;

pub use block::{
    compile_block_from_bytes, compile_block_from_bytes_with_profile, BlockEntry, BlockSnapshot,
    CompiledBlocks,
};
pub use dispatch::EntryTable;
pub use interpreter::{compile_interpreter, compile_interpreter_step};

use crate::{CpuModel, CpuState, ExecutionProfile, ExecutionTarget};

/// Generates snapshot blocks and interpreter entries for one execution target.
/// CPU observations specialize snapshot blocks; interpreters read live state.
///
/// ```
/// use relocade_x86::{Compiler, SegmentProfile};
/// let compiler = Compiler::new(SegmentProfile::Flat32);
/// let block = compiler.compile_block(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
/// let run = compiler.compile_interpreter()?;
/// let step = compiler.compile_interpreter_step()?;
/// assert_eq!(run.entry, "run");
/// assert_eq!(step.entry, "step");
/// # Ok::<(), relocade_x86::BlockError>(())
/// ```
pub struct Compiler {
    target: ExecutionTarget,
    observed_cpu: Option<CpuState>,
    code_base: Option<u32>,
    block_boundaries: std::collections::BTreeSet<u32>,
    dispatch_boundaries: bool,
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
            code_base: None,
            block_boundaries: Default::default(),
            dispatch_boundaries: false,
        }
    }

    /// Selects the same processor capabilities for blocks and interpreter entries.
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

    /// Guards external snapshot entries against a particular CS base and EIP.
    /// Direct links within the module enter after this admission check.
    #[must_use]
    pub fn with_code_base(mut self, base: u32) -> Self {
        self.code_base = Some(base);
        self
    }

    /// Stops snapshots before registered CS-relative instruction boundaries.
    #[must_use]
    pub fn with_block_boundaries(mut self, boundaries: impl IntoIterator<Item = u32>) -> Self {
        self.block_boundaries.extend(boundaries);
        self
    }

    /// Checks the shared entry table between interpreted instructions. A hit
    /// ends the current block; a miss continues decoding without checking budget.
    /// Initial entry bypasses lookup so a failed compiled guard can make progress.
    #[must_use]
    pub fn with_dispatch_boundaries(mut self) -> Self {
        self.dispatch_boundaries = true;
        self
    }

    /// The complete contract shared by every entry compiled by this owner.
    pub fn target(&self) -> ExecutionTarget {
        self.target
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
