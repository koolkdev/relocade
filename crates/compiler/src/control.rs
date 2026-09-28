//! Typed branches and direct construction of control-flow edges.
use crate::{
    body::{BlockId, Edge, Exit},
    function::PendingBlock,
    results, Arguments, BlockBuilder, BuildError, Results, Type,
};
mod block;
mod conditional;
mod loops;
mod switch;
pub use block::Label;
pub use loops::LoopLabels;

#[derive(Clone)]
pub(super) struct JoinTarget {
    target: BlockId,
    types: Vec<Type>,
}

impl BlockBuilder<'_> {
    /// Supplies this result arm, block or loop's result and completes the builder.
    /// Nested ordinary branches use an enclosing typed label instead.
    pub fn yield_(self, arguments: impl Into<Arguments>) -> Result<(), BuildError> {
        self.terminate(|body| {
            let target = body.yield_target()?;
            let arguments = body.result_arguments(arguments, &target.types)?;
            Ok(Exit::Jump(Edge {
                target: target.target,
                arguments,
            }))
        })
    }
    fn yield_target(&self) -> Result<JoinTarget, BuildError> {
        self.yield_target.clone().ok_or(BuildError::InvalidYield)
    }
    fn result_target<R: Results>(&self) -> Result<JoinTarget, BuildError> {
        let types = results::types::<R>();
        Ok(JoinTarget {
            target: self.arena.block(self.pending.id, &types)?,
            types,
        })
    }
    fn build_branch(
        &mut self,
        target: Option<&JoinTarget>,
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<PendingBlock, BuildError> {
        let scope = self.arena.child_scope(self.pending.id)?;
        let block = self.arena.block(scope, &[])?;
        self.build_block(scope, block, target, build)
    }
    fn build_block(
        &mut self,
        scope: usize,
        block: BlockId,
        target: Option<&JoinTarget>,
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<PendingBlock, BuildError> {
        let mut pending = PendingBlock::new(scope, block);
        build(BlockBuilder {
            program: self.program,
            function: self.function,
            arena: self.arena.clone(),
            pending: &mut pending,
            yield_target: target.cloned(),
        })?;
        let block = pending.finish()?;
        if block.falls_through() && target.is_some_and(|target| !target.types.is_empty()) {
            return Err(BuildError::MissingBranchValue);
        }
        Ok(block)
    }
    fn connect_fallthrough(&self, block: &PendingBlock, target: BlockId) -> Result<(), BuildError> {
        if block.falls_through() {
            self.arena.exit(
                block.current,
                Exit::Jump(Edge {
                    target,
                    arguments: Vec::new(),
                }),
            )?;
        }
        Ok(())
    }
    fn join_outputs(
        &self,
        target: &JoinTarget,
        branches: &[BlockId],
    ) -> Result<Vec<usize>, BuildError> {
        self.arena.complete_join(target.target, branches)
    }
}
#[cfg(test)]
mod tests;
