//! Configuration and construction of snapshot and runtime-decoded execution entries.

mod block;
mod interpreter;
mod interrupts;

pub use block::{compile_block_from_bytes, compile_block_from_bytes_with_profile};
pub use interpreter::{compile_interpreter, compile_interpreter_step};
pub use interrupts::compile_real_mode_interrupt;

use crate::{CpuState, ExecutionProfile};

/// Generates snapshot blocks and interpreter entries under one execution profile.
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
    profile: ExecutionProfile,
    observed_cpu: Option<CpuState>,
}

impl Compiler {
    /// Selects the segment assumptions for every entry compiled by this owner.
    pub fn new(profile: impl Into<ExecutionProfile>) -> Self {
        Self {
            profile: profile.into(),
            observed_cpu: None,
        }
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
