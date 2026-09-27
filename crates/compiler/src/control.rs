//! Structured blocks, result joins and branch construction.
use crate::{
    results, Arguments, BlockBuilder, Body, BuildError, Operation, Results, Terminal, Type,
};

mod block;
mod conditional;
mod fold;
mod loops;
mod switch;
mod tree;

pub use block::Label;
pub use loops::LoopLabels;
pub(crate) use tree::BlockTree;

pub(super) struct SwitchCase {
    pub(super) key: u32,
    pub(super) block: Block,
}

impl Body {
    pub(super) fn operation(&self, site: Site) -> &Operation {
        &self
            .block
            .walk()
            .find(|block| block.id == site.block)
            .expect("an operation result names an attached block")
            .operations[site.index]
    }
}

impl Operation {
    pub(super) fn children(&self) -> impl DoubleEndedIterator<Item = &Block> {
        let (first, second, cases): (_, _, &[SwitchCase]) = match self {
            Self::Block { block, .. } | Self::Loop { block, .. } => (Some(block), None, &[]),
            Self::BranchIf { taken, .. } => (Some(taken), None, &[]),
            Self::If {
                branch,
                else_branch,
                ..
            } => (Some(branch), else_branch.as_ref(), &[]),
            Self::Switch { cases, default, .. } => (Some(default), None, cases),
            _ => (None, None, &[]),
        };
        cases
            .iter()
            .map(|case| &case.block)
            .chain(first)
            .chain(second)
    }

    pub(super) fn branch_outputs(&self) -> &[usize] {
        match self {
            Self::Block { outputs, .. }
            | Self::Loop { outputs, .. }
            | Self::If { outputs, .. }
            | Self::Switch { outputs, .. } => outputs,
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Site {
    pub(super) block: usize,
    pub(super) index: usize,
}

/// A loop's header and result join occupy the same authored control site.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Target {
    pub(super) site: Site,
    pub(super) entry: bool,
}

impl Target {
    pub(super) fn exit(site: Site) -> Self {
        Self { site, entry: false }
    }

    pub(super) fn entry(site: Site) -> Self {
        Self { site, entry: true }
    }
}

pub(super) struct Block {
    pub(super) id: usize,
    pub(super) operations: Vec<Operation>,
    pub(super) terminal: Option<Terminal>,
}

impl Block {
    pub(super) fn walk(&self) -> Blocks<'_> {
        Blocks(vec![self])
    }

    pub(super) fn exits_to(&self, target: Target) -> impl Iterator<Item = (Site, &[usize])> {
        self.walk().filter_map(move |block| match &block.terminal {
            Some(Terminal::Branch {
                target: destination,
                arguments,
            }) if *destination == target => Some((
                Site {
                    block: block.id,
                    index: block.operations.len(),
                },
                arguments.as_slice(),
            )),
            _ => None,
        })
    }
}

pub(super) struct Blocks<'a>(Vec<&'a Block>);

impl<'a> Iterator for Blocks<'a> {
    type Item = &'a Block;
    fn next(&mut self) -> Option<Self::Item> {
        let block = self.0.pop()?;
        for operation in block.operations.iter().rev() {
            self.0.extend(operation.children().rev());
        }
        Some(block)
    }
}

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
