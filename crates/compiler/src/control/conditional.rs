//! Conditional execution and typed result selection.
use super::{branches::Branches, JoinTarget};
use crate::{
    body::{Edge, Exit},
    function::PendingBlock,
    Arguments, BlockBuilder, BuildError, Results, Val, I1,
};

impl BlockBuilder<'_> {
    /// Conditionally supplies this direct result arm, block or loop's result.
    /// A true condition yields and skips the remaining body; false leaves this
    /// builder open. The result shape and scope rules match [`Self::yield_`].
    /// Ordinary nested `if_` arms have no yield target; use [`Self::branch_if`]
    /// with an enclosing label there. Arguments are demanded on the taken path,
    /// subject to the usual snapshot rules. All inputs are checked even for a
    /// false condition, and errors leave this builder usable.
    pub fn yield_if(
        &mut self,
        condition: impl Into<Val<I1>>,
        arguments: impl Into<Arguments>,
    ) -> Result<(), BuildError> {
        let target = self.yield_target()?;
        self.conditional_branch(condition, target, arguments)
    }

    pub(super) fn conditional_branch(
        &mut self,
        condition: impl Into<Val<I1>>,
        target: JoinTarget,
        arguments: impl Into<Arguments>,
    ) -> Result<(), BuildError> {
        let condition = self.operand(condition)?;
        let arguments = self.result_arguments(arguments, &target.types)?;
        let condition = self.arena.normalize(condition)?;
        self.conditional(
            condition,
            None,
            |branch| {
                branch.terminate(|_| {
                    Ok(Exit::Jump(Edge {
                        target: target.target,
                        arguments,
                    }))
                })
            },
            |_| Ok(()),
        )?;
        Ok(())
    }

    /// Builds a branch that executes when the condition is true. A false condition
    /// skips it. The child has the same load, store, conditional and return methods.
    /// Return `Ok(())` without a terminal to continue after the branch.
    /// A closure error discards the branch and leaves the parent usable.
    /// A condition settled by a constant or an earlier test on this path invokes
    /// only the reachable closure. Skipped closures are not validated and must not
    /// be relied upon for construction-time side effects. Otherwise both closures
    /// of a conditional are built and checked.
    ///
    /// Values depending on child reads, calls or joins can be consumed only in
    /// that child or its descendants. Pure expressions from parent values can be
    /// used on either path.
    ///
    /// ```
    /// use relocade_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I32],
    /// }, |mut body| {
    ///     let value = body.parameter::<I32>(0)?;
    ///     body.if_(value.eq(0), |branch| branch.return_(7))?;
    ///     body.return_(value.add(1))
    /// })?;
    /// program.export("increment_or_seven", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), relocade_compiler::BuildError>(())
    /// ```
    pub fn if_(
        &mut self,
        condition: impl Into<Val<I1>>,
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        self.if_else(condition, build, |_| Ok(()))
    }

    /// Executes exactly one of two branches. Each branch may fall through,
    /// return from the function, tail-call or trap. A construction error discards both
    /// branches and leaves the parent usable. Child values follow `if_`'s scope rules.
    /// Only closures reachable under known conditions run, as described by [`Self::if_`].
    ///
    /// ```
    /// use relocade_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I32],
    /// }, |mut body| {
    ///     let value = body.parameter::<I32>(0)?;
    ///     body.if_else(value.eq(0),
    ///         |branch| branch.return_(7),
    ///         |_| Ok(()),
    ///     )?;
    ///     body.return_(value.add(1))
    /// })?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), relocade_compiler::BuildError>(())
    /// ```
    pub fn if_else(
        &mut self,
        condition: impl Into<Val<I1>>,
        then_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
        else_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let condition = self.operand(condition)?;
        let condition = self.arena.normalize(condition)?;
        self.conditional(condition, None, then_build, else_build)?;
        Ok(())
    }

    /// Selects a typed result by executing one of two branches. Nonempty result
    /// arms must consume their builder with `yield_`, an outward `branch`,
    /// `return_`, `tail_call` or `trap`. If every constructed arm exits elsewhere,
    /// the result and continuation are unreachable. Unit result arms may fall through.
    /// A yield supplies this conditional's result, while a return exits the function.
    /// A construction error discards both arms and leaves the parent usable.
    /// Only closures reachable under known conditions run, as described by [`Self::if_`].
    ///
    /// The selected value is visible in the parent. Other values depending on
    /// child reads, calls or joins remain confined to that child and its descendants.
    ///
    /// ```
    /// use relocade_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I32],
    /// }, |mut body| {
    ///     let value = body.parameter::<I32>(0)?;
    ///     let selected = body.if_value::<I32>(value.eq(0),
    ///         |arm| arm.yield_(7),
    ///         |arm| arm.yield_(value.add(1)),
    ///     )?;
    ///     body.return_(selected.add(2))
    /// })?;
    /// program.export("choose_then_add", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), relocade_compiler::BuildError>(())
    /// ```
    pub fn if_value<R: Results>(
        &mut self,
        condition: impl Into<Val<I1>>,
        then_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
        else_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError> {
        let condition = self.operand(condition)?;
        let condition = self.arena.normalize(condition)?;
        let target = self.result_target::<R>()?;
        let outputs = self.conditional(condition, Some(target), then_build, else_build)?;
        Ok(crate::results::bind::<R>(self, &outputs))
    }

    fn conditional(
        &mut self,
        condition: usize,
        target: Option<JoinTarget>,
        then_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
        else_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Vec<usize>, BuildError> {
        let branches = match self.arena.condition(&self.pending.path, condition)? {
            Some(true) => Branches::Scope(self.build_branch(target.as_ref(), then_build)?),
            Some(false) => Branches::Scope(self.build_branch(target.as_ref(), else_build)?),
            None => Branches::If {
                condition,
                arms: [
                    self.build_conditional_arm(condition, true, target.as_ref(), then_build)?,
                    self.build_conditional_arm(condition, false, target.as_ref(), else_build)?,
                ],
            },
        };
        branches.finish(self, target)
    }

    fn build_conditional_arm(
        &mut self,
        condition: usize,
        truth: bool,
        target: Option<&JoinTarget>,
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<PendingBlock, BuildError> {
        let path = self.arena.assume(&self.pending.path, condition, truth)?;
        self.build_branch(target, |arm| {
            arm.pending.path = path;
            build(arm)
        })
    }
}
