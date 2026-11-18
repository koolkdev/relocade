//! Bounded discovery of guest blocks from requested entry addresses.

mod walk;

#[cfg(test)]
mod tests;

use super::{block::module::BlockModule, BlockCompileError, CompiledBlocks, Compiler};
use crate::{BlockError, CodeAddress, SegmentDefaultSize};
use relocade_compiler::CompileExecutor;
use std::collections::BTreeMap;

/// Nonzero limits for discovery from one root. Its instruction and byte budgets
/// count only newly accepted instructions. Reaching a known instruction ends that
/// path and starts a block at that instruction. Exceeding either budget
/// rejects the root and preserves previously accepted blocks.
#[derive(Clone, Copy, Debug)]
pub struct DiscoveryLimits {
    /// Maximum number of new instructions this root can add.
    pub instructions: u32,
    /// Maximum total encoding bytes of those instructions, excluding read-ahead.
    pub bytes: usize,
    /// Maximum instructions per emitted block. At this limit discovery
    /// continues the straight-line path in a new block.
    pub block_instructions: u32,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            instructions: 64 * 1024,
            bytes: 1024 * 1024,
            block_instructions: 256,
        }
    }
}

impl Compiler {
    /// Begins discovery over one stable guest code source. The reader copies a
    /// contiguous readable prefix from each address and returns its length.
    /// Zero ends that path. The reader owns address translation and must avoid MMIO.
    pub fn discover_blocks<R>(&self, read: R) -> BlockDiscovery<'_, R>
    where
        R: FnMut(CodeAddress, &mut [u8]) -> usize,
    {
        BlockDiscovery {
            compiler: self,
            views: BTreeMap::new(),
            read,
        }
    }

    /// Discovers and compiles blocks from one CS-relative root using the profile's
    /// default code width. See [`BlockDiscovery`] for transfer and export behavior.
    pub fn compile_blocks_from_root(
        &self,
        start_eip: u32,
        limits: DiscoveryLimits,
        read: impl FnMut(u32, &mut [u8]) -> usize,
    ) -> Result<CompiledBlocks, BlockError> {
        self.discover_from_root(start_eip, limits, read)?.compile()
    }

    /// Compiles the same blocks as [`Self::compile_blocks_from_root`] on the supplied executor.
    pub async fn compile_blocks_from_root_with<E: CompileExecutor>(
        &self,
        start_eip: u32,
        limits: DiscoveryLimits,
        read: impl FnMut(u32, &mut [u8]) -> usize,
        executor: &E,
    ) -> Result<CompiledBlocks, BlockCompileError<E::Error>> {
        self.discover_from_root(start_eip, limits, read)?
            .compile_with(executor)
            .await
    }

    fn discover_from_root(
        &self,
        start: u32,
        limits: DiscoveryLimits,
        mut read: impl FnMut(u32, &mut [u8]) -> usize,
    ) -> Result<BlockModule<'_>, BlockError> {
        let size = SegmentDefaultSize::for_profile(self.target.profile);
        let mut discovery = self.discover_blocks(|address, bytes| read(address.eip(), bytes));
        discovery.add_root(CodeAddress::CsRelative(start), size, limits)?;
        discovery.into_module()
    }
}

/// Discovers blocks shared by roots in the same code segment and decode width.
/// Direct jumps, conditional branches and fallthroughs can link to any block
/// found in that view. Calls use runtime bindings. Returns dispatch to the
/// live guest return address. The CPU can enter at each decoded requested root
/// and each decoded continuation after a near CALL or Real16 far CALL.
///
/// Keep the code source and its mappings stable throughout discovery. Direct
/// links require valid code, compatible execution state and no intervening host
/// hooks throughout execution. [`CompiledBlocks::spans`] covers every emitted
/// block, including internal jump targets.
pub struct BlockDiscovery<'a, R> {
    compiler: &'a Compiler,
    views: BTreeMap<(Option<u16>, SegmentDefaultSize), walk::Discovery>,
    read: R,
}

impl<'a, R: FnMut(CodeAddress, &mut [u8]) -> usize> BlockDiscovery<'a, R> {
    /// Explores direct branches and known fallthroughs from this entry. Discovery
    /// follows the return continuation of a supported call in the caller's code
    /// segment. Request a callee as a root to compile it. Returns, indirect
    /// jumps and code-context changes end that path.
    ///
    /// An accepted instruction ends exploration along that edge before any
    /// further reads or charges to this root. Unreadable paths end quietly.
    /// An undecodable root returns its decoding error. A later decoding failure
    /// ends only that path. Exceeding an instruction or byte budget returns
    /// [`BlockError::DiscoveryTooLarge`] and leaves earlier roots intact.
    pub fn add_root(
        &mut self,
        entry: CodeAddress,
        code_default_size: SegmentDefaultSize,
        limits: DiscoveryLimits,
    ) -> Result<(), BlockError> {
        let view = self
            .views
            .entry((entry.segment(), code_default_size))
            .or_insert_with(|| {
                walk::Discovery::new(entry, code_default_size, self.compiler.target)
            });
        view.add_root(entry.eip(), limits, |eip, bytes| {
            (self.read)(entry.with_eip(eip), bytes)
        })
    }

    /// True until at least one root has contributed instructions.
    pub fn is_empty(&self) -> bool {
        self.views.values().all(walk::Discovery::is_empty)
    }

    fn into_module(self) -> Result<BlockModule<'a>, BlockError> {
        let mut module = BlockModule::new(self.compiler);
        for view in self.views.into_values() {
            module.append(view.finish())?;
        }
        Ok(module)
    }

    /// Emits all discovered blocks and exports the roots and return continuations.
    /// Exporting the same address at two decode widths requires separate modules.
    pub fn compile(self) -> Result<CompiledBlocks, BlockError> {
        self.into_module()?.compile()
    }

    /// Emits the same blocks and exports using the supplied compile executor.
    pub async fn compile_with<E: CompileExecutor>(
        self,
        executor: &E,
    ) -> Result<CompiledBlocks, BlockCompileError<E::Error>> {
        self.into_module()?.compile_with(executor).await
    }
}
