//! Public snapshot compilation inputs, outputs and entry points.

mod decode;
mod error;
pub(super) mod module;

pub use decode::DecodedBlock;
pub use error::BlockCompileError;

use super::{CodeAddress, Compiler};
use crate::{
    BlockError, CompiledModule, ExecutionProfile, ExecutionTarget, SegmentDefaultSize,
    SegmentProfile,
};
use module::BlockModule;
use relocade_compiler::CompileExecutor;

/// Captured instruction bytes and a compilation bound for one CS-relative entry.
/// The bytes must match readable guest memory when the resulting block executes.
#[derive(Clone, Copy, Debug)]
pub struct BlockSnapshot<'a> {
    pub address: CodeAddress,
    /// Loaded CS.D used to decode these bytes.
    pub code_default_size: SegmentDefaultSize,
    pub bytes: &'a [u8],
    /// Nonzero maximum number of instructions to decode from this snapshot.
    pub instruction_limit: u32,
}

/// One exported block and the instruction bytes on which it depends.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockEntry {
    /// Guest location at which the snapshot was taken.
    pub address: CodeAddress,
    /// CS.D used to decode the captured instructions.
    pub code_default_size: SegmentDefaultSize,
    /// Bytes consumed from the snapshot, excluding any unused suffix.
    /// Offsets advance from the entry EIP with 32-bit wrapping.
    pub byte_len: usize,
    /// Name of the export using [`crate::abi::ExecutionSignature`].
    pub export: String,
}

/// Guest blocks sharing one Wasm module, its imports and generated helpers.
pub struct CompiledBlocks {
    pub bytes: Vec<u8>,
    /// CPU-callable exports. Explicit snapshots preserve the supplied order.
    pub entries: Vec<BlockEntry>,
    /// Guest address and byte length of every block, including private jump targets.
    /// Their bytes and instruction-fetch mappings must remain valid while the module can run.
    pub spans: Vec<(CodeAddress, usize)>,
    /// Unique packed Real16 CS:IP targets, in call-table slot order.
    /// Calls to the same target share a slot throughout the module.
    /// Unresolved slots initially call `relocade.resolveAndJump`.
    pub call_targets: Vec<u32>,
    /// Shared execution policy. Each entry records its own decode width.
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
    /// Decodes explicit snapshots up to the boundaries documented by [`Self::compile_blocks`].
    /// Every supplied address bounds decoding, including entries the caller later omits.
    pub fn decode_blocks(
        &self,
        snapshots: &[BlockSnapshot<'_>],
    ) -> Result<Vec<DecodedBlock>, BlockError> {
        decode::blocks(snapshots, self.target)
    }

    /// Lowers caller-supplied blocks without reading guest memory or discovering successors.
    ///
    /// Known jumps and fallthroughs link to supplied blocks in the same code segment
    /// and decode width, including private blocks. Calls use replaceable Real16 slots
    /// or the runtime resolver. Returns dispatch to the live guest destination.
    /// Every edge retains its execution checkpoint and destination entry guards.
    ///
    /// Each block must end at or before the first execution boundary for this target.
    /// A code segment and decode width may contain only one block per start address.
    /// Exports require unique guest addresses across all widths. The caller chooses
    /// the public entries and maintains code and mapping validity for all source spans
    /// as described in the [host integration contract](crate#entry-validity).
    pub fn compile_decoded_blocks(
        &self,
        blocks: Vec<DecodedBlock>,
    ) -> Result<CompiledBlocks, BlockError> {
        BlockModule::new(self).build_decoded(blocks)?.compile()
    }

    /// Lowers the same blocks as [`Self::compile_decoded_blocks`] on the supplied executor.
    pub async fn compile_decoded_blocks_with<E: CompileExecutor>(
        &self,
        blocks: Vec<DecodedBlock>,
        executor: &E,
    ) -> Result<CompiledBlocks, BlockCompileError<E::Error>> {
        BlockModule::new(self)
            .build_decoded(blocks)?
            .compile_with(executor)
            .await
    }

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
            address: CodeAddress::CsRelative(start_eip),
            code_default_size: SegmentDefaultSize::for_profile(self.target.profile),
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
    /// An empty slice produces a module without exports. Repeated guest addresses
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
    /// Every export requires its recorded CS.D, original EIP and valid snapshot bytes under
    /// the current CS base. The embedding admits entries against
    /// [`CompiledBlocks::execution_target`] and maintains their validity throughout
    /// execution, as described by the [host integration contract](crate#entry-validity).
    /// The caller must also limit blocks at registered execution boundaries absent
    /// from this batch.
    ///
    /// ```
    /// use relocade_x86::{BlockSnapshot, CodeAddress, Compiler, SegmentDefaultSize, SegmentProfile};
    /// let compiler = Compiler::new(SegmentProfile::Flat32);
    /// let code_default_size = SegmentDefaultSize::Bits32;
    /// let module = compiler.compile_blocks(&[
    ///     BlockSnapshot {
    ///         address: CodeAddress::CsRelative(0x1000), code_default_size, bytes: &[0xb8, 42, 0, 0, 0], instruction_limit: 1,
    ///     },
    ///     BlockSnapshot {
    ///         address: CodeAddress::CsRelative(0x2000), code_default_size, bytes: &[0xeb, 0xfe], instruction_limit: 1,
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
        BlockModule::new(self).build(snapshots)?.compile()
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
        BlockModule::new(self)
            .build(snapshots)?
            .compile_with(executor)
            .await
    }
}
