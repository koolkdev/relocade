//! Public snapshot compilation inputs, outputs and entry points.

pub(super) mod decode;
mod error;
pub(super) mod module;

pub use error::BlockCompileError;

use super::Compiler;
use crate::{BlockError, CompiledModule, ExecutionProfile, ExecutionTarget, SegmentProfile};
use module::BlockModule;
use relocade_compiler::CompileExecutor;

/// Captured instruction bytes and a compilation bound for one CS-relative entry.
/// The bytes must match readable guest memory when the resulting block executes.
#[derive(Clone, Copy, Debug)]
pub struct BlockSnapshot<'a> {
    pub start_eip: u32,
    pub bytes: &'a [u8],
    /// Nonzero maximum number of instructions to decode from this snapshot.
    pub instruction_limit: u32,
}

/// One exported block and the instruction bytes on which it depends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockEntry {
    /// CS-relative EIP required when entering this export.
    pub start_eip: u32,
    /// Bytes consumed from the snapshot, excluding any unused suffix.
    /// Offsets advance from `start_eip` with 32-bit wrapping.
    pub byte_len: usize,
    /// Name of the export using [`crate::abi::ExecutionSignature`].
    pub export: String,
}

/// One constant-index destination in the module's `relocade.callTargets` table.
/// Near destinations inherit CS from `entries[caller_entry]`; far destinations
/// supply it explicitly. Only discovered Real16 functions currently emit links.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallLink {
    Near { caller_entry: usize, eip: u32 },
    Far { selector: u16, offset: u16 },
}

/// Snapshot blocks sharing one Wasm module, its imports and generated helpers.
/// Entry metadata follows the supplied snapshot order.
pub struct CompiledBlocks {
    pub bytes: Vec<u8>,
    pub entries: Vec<BlockEntry>,
    /// Ordered destinations for a private function table on each installation.
    /// Unresolved slots initially call `relocade.resolveAndJump`.
    pub calls: Vec<CallLink>,
    /// Required target for every entry and its interpreter fallback.
    /// The host also maintains each entry's code bytes and mapping validity.
    pub execution_target: ExecutionTarget,
}

/// Compiles a byte snapshot under [`SegmentProfile::Flat32`].
///
/// Compilation stops at the first branch, segment load, POPF/POPFD, port I/O,
/// serializing instruction, unconditional fault or `instruction_limit`. A conditional
/// branch ends the block on both outcomes; bytes after the boundary are ignored.
/// The limit must be nonzero. Incomplete,
/// unsupported or overlong instructions within that boundary return [`BlockError`].
///
/// The generated `block_<hex start_eip>() -> ExecutionExit` entry executes the block and
/// tail-calls host dispatch on success. Supported Real16 faults enter their IVT
/// handler and return `InterruptEntered`; other faults return directly.
/// A failed guard publishes completed work and tail-calls
/// `relocade.interpret` at the current instruction. The interpreter handoff abandons
/// the captured suffix; its execution boundary belongs to the host's policy.
/// The host must satisfy the profile and snapshot-validity requirements documented
/// by [`compile_block_from_bytes_with_profile`].
///
/// ```
/// use relocade_x86::compile_block_from_bytes;
/// let module = compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
/// assert_eq!(module.entry, "block_1000");
/// # Ok::<(), relocade_x86::BlockError>(())
/// ```
pub fn compile_block_from_bytes(
    start_eip: u32,
    bytes: &[u8],
    instruction_limit: u32,
) -> Result<CompiledModule, BlockError> {
    compile_block_from_bytes_with_profile(
        start_eip,
        bytes,
        instruction_limit,
        SegmentProfile::Flat32,
    )
}

/// Compiles a byte snapshot under the selected execution profile.
/// The stopping boundary and exported entry follow [`compile_block_from_bytes`].
/// The profile determines instruction defaults; size prefixes independently
/// select the opposite operand or address width.
///
/// The block checks its complete compiled fetch span against CS at entry. A failed
/// check enters the interpreter before executing any instruction, so valid prefixes
/// and faults retain their architectural order. Profile-proven checks fold away.
/// The host must establish profile compatibility and ensure the snapshot matches
/// readable guest memory at the current CS base. Changes to code bytes or mappings
/// still require revalidation or invalidation of affected entries and direct links.
/// See the [host integration contract](crate#entry-validity) for validity throughout
/// execution.
///
/// ```
/// use relocade_x86::{compile_block_from_bytes_with_profile, Compiler, SegmentProfile};
///
/// // MOV AX, 0x1234 under 16-bit code defaults.
/// let block = compile_block_from_bytes_with_profile(
///     0x1000, &[0xb8, 0x34, 0x12], 1, SegmentProfile::Segmented16,
/// )?;
/// assert_eq!(block.execution_target, Some(Compiler::new(SegmentProfile::Segmented16).target()));
/// # Ok::<(), relocade_x86::BlockError>(())
/// ```
pub fn compile_block_from_bytes_with_profile(
    start_eip: u32,
    bytes: &[u8],
    instruction_limit: u32,
    profile: impl Into<ExecutionProfile>,
) -> Result<CompiledModule, BlockError> {
    Compiler::new(profile).compile_block(start_eip, bytes, instruction_limit)
}

impl Compiler {
    /// Compiles a snapshot through the boundary described by
    /// [`compile_block_from_bytes`], with this compiler's execution target and
    /// observations. Fetch admission follows [`compile_block_from_bytes_with_profile`].
    pub fn compile_block(
        &self,
        start_eip: u32,
        bytes: &[u8],
        instruction_limit: u32,
    ) -> Result<CompiledModule, BlockError> {
        let mut blocks = self.compile_blocks(&[BlockSnapshot {
            start_eip,
            bytes,
            instruction_limit,
        }])?;
        let entry = blocks
            .entries
            .pop()
            .expect("one snapshot exports one block");
        Ok(CompiledModule {
            bytes: blocks.bytes,
            entry: entry.export,
            execution_target: Some(blocks.execution_target),
        })
    }

    /// Compiles independently callable snapshot blocks into one module.
    /// Entries share the compiler's target, observations, imports and helpers.
    /// An empty slice produces a module without exports; repeated starting EIPs
    /// return [`BlockError::DuplicateBlock`].
    ///
    /// Each block stops at the ordinary [`compile_block_from_bytes`] boundary,
    /// or before another supplied entry reached at an instruction boundary.
    /// Entry points inside an instruction's encoding do not split that instruction.
    /// Unused snapshot bytes are neither decoded nor included in its [`BlockEntry`].
    /// Success publishes state, checks execution interrupts, then tail-calls
    /// `relocade.dispatch`, including when the successor is in this module.
    /// Faults and interpreter handoffs retain the single-block contract.
    ///
    /// Every export requires its original EIP and valid snapshot bytes under
    /// the current CS base. The embedding admits entries against
    /// [`CompiledBlocks::execution_target`] and maintains their validity throughout
    /// execution, as described by the [host integration contract](crate#entry-validity).
    /// The caller must also limit blocks at registered execution boundaries absent
    /// from this batch.
    ///
    /// ```
    /// use relocade_x86::{BlockSnapshot, Compiler, SegmentProfile};
    /// let compiler = Compiler::new(SegmentProfile::Flat32);
    /// let module = compiler.compile_blocks(&[
    ///     BlockSnapshot {
    ///         start_eip: 0x1000, bytes: &[0xb8, 42, 0, 0, 0], instruction_limit: 1,
    ///     },
    ///     BlockSnapshot {
    ///         start_eip: 0x2000, bytes: &[0xeb, 0xfe], instruction_limit: 1,
    ///     },
    /// ])?;
    /// assert_eq!(module.entries[0].export, "block_1000");
    /// assert_eq!(module.entries[0].byte_len, 5);
    /// assert_eq!(module.entries[1].export, "block_2000");
    /// assert_eq!(module.execution_target, compiler.target());
    /// # Ok::<(), relocade_x86::BlockError>(())
    /// ```
    pub fn compile_blocks(
        &self,
        snapshots: &[BlockSnapshot<'_>],
    ) -> Result<CompiledBlocks, BlockError> {
        let blocks = BlockModule::build(self, decode::blocks(snapshots, self.target)?)?;
        Ok(CompiledBlocks {
            bytes: blocks.program.compile()?,
            entries: blocks.entries,
            calls: blocks.calls,
            execution_target: self.target,
        })
    }

    /// Compiles snapshot blocks using the caller's executor for independent jobs.
    /// Uses the boundaries, entry contract and result of [`Self::compile_blocks`].
    /// The executor controls admission, concurrency and worker lifetime through
    /// [`CompileExecutor`]. Block construction errors and executor failures remain
    /// distinct in [`BlockCompileError`].
    pub async fn compile_blocks_with<E: CompileExecutor>(
        &self,
        snapshots: &[BlockSnapshot<'_>],
        executor: &E,
    ) -> Result<CompiledBlocks, BlockCompileError<E::Error>> {
        let blocks = BlockModule::build(self, decode::blocks(snapshots, self.target)?)?;
        Ok(CompiledBlocks {
            bytes: blocks.program.compile_with(executor).await?,
            entries: blocks.entries,
            calls: blocks.calls,
            execution_target: self.target,
        })
    }
}
