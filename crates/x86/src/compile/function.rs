//! Bounded discovery from a guest function entry, using the snapshot block emitter.

mod discover;

#[cfg(test)]
mod tests;

use super::{block::module::BlockModule, BlockCompileError, CompiledBlocks, Compiler};
use crate::BlockError;
use relocade_compiler::CompileExecutor;

/// Total work admitted for one function candidate, including all discovered paths.
/// Reaching a bound leaves remaining execution to ordinary dispatch.
#[derive(Clone, Copy, Debug)]
pub struct FunctionLimits {
    /// Maximum number of retained decoded instructions.
    pub instructions: u32,
    /// Maximum total encoding bytes, excluding unused read-ahead bytes.
    pub bytes: usize,
    /// Maximum entry points considered, including unreadable destinations.
    pub blocks: usize,
}

impl Default for FunctionLimits {
    fn default() -> Self {
        Self {
            instructions: 1024,
            bytes: 16 * 1024,
            blocks: 64,
        }
    }
}

impl Compiler {
    /// Discovers and compiles a bounded region rooted at a guest function entry.
    /// Follows direct jumps, both conditional successors and near-call continuations.
    /// Callees, indirect jumps, returns and other execution boundaries end a path.
    /// This does not prove source-language function boundaries or that calls return.
    ///
    /// `read` copies a contiguous readable prefix at a CS-relative EIP into the
    /// supplied buffer and returns its length; zero ends that path. It owns address
    /// translation and must not access MMIO. Reads may include an unused suffix.
    /// The caller supplies clean function candidates with no interior host hooks
    /// or other registered execution boundaries. An edge into an instruction's
    /// encoding is left to dispatch.
    ///
    /// Entries are returned in increasing EIP order. Each instruction is decoded
    /// once and retained through block splitting and IR construction.
    /// An incomplete or unsupported instruction fails this request;
    /// reaching a work bound or unreadable successor is a normal discovery boundary.
    /// An unreadable entry or insufficient byte budget can produce no blocks.
    ///
    /// Exports, dispatch, guards and code/addressing validity have the same contract
    /// as [`Self::compile_blocks`]. The caller keeps code and mappings stable during
    /// discovery and maintains validity for the resulting entries' lifetime.
    pub fn compile_function(
        &self,
        start_eip: u32,
        limits: FunctionLimits,
        read: impl FnMut(u32, &mut [u8]) -> usize,
    ) -> Result<CompiledBlocks, BlockError> {
        let decoded = discover::blocks(start_eip, self.target, limits, read)?;
        let blocks = BlockModule::build(self, decoded)?;
        Ok(CompiledBlocks {
            bytes: blocks.program.compile()?,
            entries: blocks.entries,
            execution_target: self.target,
        })
    }

    /// Uses the same discovery and entry contract as [`Self::compile_function`],
    /// then runs compilation jobs on the supplied executor.
    pub async fn compile_function_with<E: CompileExecutor>(
        &self,
        start_eip: u32,
        limits: FunctionLimits,
        read: impl FnMut(u32, &mut [u8]) -> usize,
        executor: &E,
    ) -> Result<CompiledBlocks, BlockCompileError<E::Error>> {
        let decoded = discover::blocks(start_eip, self.target, limits, read)?;
        let blocks = BlockModule::build(self, decoded)?;
        Ok(CompiledBlocks {
            bytes: blocks.program.compile_with(executor).await?,
            entries: blocks.entries,
            execution_target: self.target,
        })
    }
}
