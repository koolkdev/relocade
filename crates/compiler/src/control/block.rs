//! Typed outward exits from structured blocks.
use std::marker::PhantomData;

use super::Site;
use crate::{
    arena::ExpressionArena, Arguments, BuildError, FunctionBuilder, Operation, Results, Terminal,
};

/// The typed exit of a block, usable only within that block and its descendants.
/// A label belongs to one function body. Leaving or discarding its block makes
/// it unavailable; cloning a label does not extend that scope.
pub struct Label<R: Results> {
    arena: ExpressionArena,
    scope: usize,
    target: Site,
    shape: PhantomData<fn() -> R>,
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

impl FunctionBuilder<'_> {
    /// Builds a block with a typed result and an outward exit label.
    /// Descendants may pass values to the label with [`Self::branch`], skipping
    /// the remainder of the block. Its direct body may instead use [`Self::yield_`].
    /// Each reachable completion must supply the result, branch to an enclosing
    /// block, or exit the function; a unit block may fall through. A nonempty result
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
        build: impl FnOnce(FunctionBuilder<'_>, Label<R>) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError> {
        let target = self.result_target::<R>();
        let scope = self.arena.child_scope(self.region.id)?;
        let label = Label {
            arena: self.arena.clone(),
            scope,
            target: target.site,
            shape: PhantomData,
        };
        let region = self.build_region(scope, Some(&target), |body| build(body, label))?;
        let outputs = self.join_outputs(&target, [&region])?;
        let values = crate::results::bind::<R>(self, &outputs);
        self.region
            .operations
            .push(Operation::Block { region, outputs });
        Ok(values)
    }

    /// Leaves the named enclosing block with its declared result, consuming the
    /// active builder. The result shape validates literals and typed values as
    /// with [`Self::yield_`]. A parent, sibling or another body cannot use the label.
    pub fn branch<R: Results>(
        mut self,
        label: &Label<R>,
        arguments: impl Into<Arguments>,
    ) -> Result<(), BuildError> {
        self.fallthrough = false;
        if !self.arena.same_body(&label.arena) {
            return Err(BuildError::ForeignBody);
        }
        self.arena.require_scope(label.scope, self.region.id)?;
        let arguments = self.result_arguments(arguments, &crate::results::types::<R>())?;
        self.complete(Terminal::Branch {
            target: label.target,
            arguments,
        })
    }
}
