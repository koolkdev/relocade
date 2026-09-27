//! Branch construction, labels and result joins.
use crate::{
    body::{Block, Target, Terminal},
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
    target: Target,
    types: Vec<Type>,
}

impl BlockBuilder<'_> {
    /// Supplies the direct result arm, block or loop's result, consuming its builder.
    /// The declared result shape determines native literals' logical types; typed
    /// values must match it. Scalar arguments stay scalar, and tuples follow the
    /// corresponding tuple of result types. A unit result accepts `()`.
    ///
    /// Execution continues after this control operation. Only a direct result
    /// arm, block or loop body may yield. From a nested branch, use [`Self::branch`]
    /// with the enclosing block's label or the loop's `exit` label.
    pub fn yield_(self, arguments: impl Into<Arguments>) -> Result<(), BuildError> {
        self.terminate(|body| {
            let target = body.yield_target()?;
            let arguments = body.result_arguments(arguments, &target.types)?;
            Ok(Terminal::Branch {
                target: target.target,
                arguments,
            })
        })
    }

    fn yield_target(&self) -> Result<JoinTarget, BuildError> {
        self.yield_target.clone().ok_or(BuildError::InvalidYield)
    }

    fn result_target<R: Results>(&self) -> JoinTarget {
        JoinTarget {
            target: Target::exit(self.site()),
            types: results::types::<R>(),
        }
    }

    fn build_branch(
        &mut self,
        target: Option<&JoinTarget>,
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Block, BuildError> {
        let scope = self.arena.child_scope(self.pending.id)?;
        self.build_block(scope, target, build)
    }

    fn build_block(
        &mut self,
        scope: usize,
        target: Option<&JoinTarget>,
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Block, BuildError> {
        let mut pending = crate::function::PendingBlock::new(scope);
        build(BlockBuilder {
            program: self.program,
            function: self.function,
            arena: self.arena.clone(),
            pending: &mut pending,
            yield_target: target.cloned(),
        })?;
        let block = pending.finish()?;
        if block.terminal.is_none() && target.is_some_and(|target| !target.types.is_empty()) {
            return Err(BuildError::MissingBranchValue);
        }
        Ok(block)
    }

    fn join_outputs<'a>(
        &self,
        target: &JoinTarget,
        branches: impl IntoIterator<Item = &'a Block>,
    ) -> Result<Vec<usize>, BuildError> {
        let incoming: Vec<_> = branches
            .into_iter()
            .flat_map(|branch| branch.exits_to(target.target))
            .collect();
        if incoming.is_empty() && !target.types.is_empty() {
            return Err(BuildError::MissingBranchValue);
        }
        target
            .types
            .iter()
            .enumerate()
            .map(|(component, &ty)| {
                let inputs: Vec<_> = incoming
                    .iter()
                    .map(|(_, arguments)| arguments[component])
                    .collect();
                self.arena
                    .join_result(ty, target.target.site, component, &inputs)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
