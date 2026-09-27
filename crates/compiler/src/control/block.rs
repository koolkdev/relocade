//! Scoped typed labels and outward exits from structured blocks.
use std::marker::PhantomData;

use super::JoinTarget;
use crate::{
    arena::ExpressionArena,
    body::{Operation, Target, Terminal},
    Arguments, BlockBuilder, BuildError, Results, Val, I1,
};

/// A typed block exit, loop header or loop exit, usable within its control block
/// and descendants. A label belongs to one function body. Leaving or discarding
/// its block makes it unavailable; cloning a label does not extend that scope.
pub struct Label<R: Results> {
    pub(super) arena: ExpressionArena,
    pub(super) scope: usize,
    pub(super) target: Target,
    pub(super) shape: PhantomData<fn() -> R>,
}

impl<R: Results> Clone for Label<R> {
    fn clone(&self) -> Self {
        Self {
            arena: self.arena.clone(),
            scope: self.scope,
            target: self.target,
            shape: PhantomData,
        }
    }
}

impl BlockBuilder<'_> {
    /// Builds a block with a typed result and an outward exit label.
    /// Descendants may pass values to the label with [`Self::branch`], skipping
    /// the remainder of the block. Its direct body may instead use [`Self::yield_`].
    /// Each reachable completion must supply the result, branch to an enclosing
    /// control label, or exit the function; a unit block may fall through. A nonempty result
    /// requires at least one incoming result. Construction errors discard the block
    /// and keep its parent usable.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I1, I32};
    /// let mut program = Program::new();
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I1], results: vec![Type::I32],
    /// }, |mut body| {
    ///     let early = body.parameter::<I1>(0)?;
    ///     let (value, flag) = body.block::<(I32, I1)>(|mut block, exit| {
    ///         block.if_(&early, |branch| branch.branch(&exit, (7, true)))?;
    ///         block.yield_((11, false))
    ///     })?;
    ///     body.return_(value.add(flag.unsigned().extend::<I32>()))
    /// })?;
    /// program.export("choose", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn block<R: Results>(
        &mut self,
        build: impl FnOnce(BlockBuilder<'_>, Label<R>) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError> {
        let target = self.result_target::<R>();
        let scope = self.arena.child_scope(self.pending.id)?;
        let label = Label {
            arena: self.arena.clone(),
            scope,
            target: target.target,
            shape: PhantomData,
        };
        let block = self.build_block(scope, Some(&target), |body| build(body, label))?;
        let outputs = self.join_outputs(&target, [&block])?;
        let values = crate::results::bind::<R>(self, &outputs);
        self.pending
            .operations
            .push(Operation::Block { block, outputs });
        Ok(values)
    }

    /// Passes values to an enclosing control label, consuming the active builder.
    /// A block or loop exit supplies its result; a loop header starts the next
    /// iteration with the complete new input tuple. The label's shape validates
    /// literals and typed values as with [`Self::yield_`]. A parent, sibling or
    /// another body cannot use the label.
    pub fn branch<R: Results>(
        self,
        label: &Label<R>,
        arguments: impl Into<Arguments>,
    ) -> Result<(), BuildError> {
        self.terminate(|body| {
            let target = body.branch_target(label)?;
            let arguments = body.result_arguments(arguments, &target.types)?;
            Ok(Terminal::Branch {
                target: target.target,
                arguments,
            })
        })
    }

    /// Conditionally passes values to an enclosing label. A true condition skips
    /// the remaining body; false continues with this builder still open. The
    /// label and arguments follow [`Self::branch`]'s type and scope rules.
    /// Arguments are demanded on the taken path, with the usual snapshot rules.
    /// All inputs are checked even for a false condition; errors leave the parent usable.
    pub fn branch_if<R: Results>(
        &mut self,
        condition: impl Into<Val<I1>>,
        label: &Label<R>,
        arguments: impl Into<Arguments>,
    ) -> Result<(), BuildError> {
        let target = self.branch_target(label)?;
        self.conditional_branch(condition, target, arguments)
    }

    fn branch_target<R: Results>(&self, label: &Label<R>) -> Result<JoinTarget, BuildError> {
        if !self.arena.same_body(&label.arena) {
            return Err(BuildError::ForeignBody);
        }
        self.arena.require_scope(label.scope, self.pending.id)?;
        Ok(JoinTarget {
            target: label.target,
            types: crate::results::types::<R>(),
        })
    }
}
