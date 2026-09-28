//! Conditional execution and typed result selection.
use super::JoinTarget;
use crate::{
    body::{BlockId, Edge, Exit, Layout},
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
        let taken = self.build_branch(None, |branch| {
            branch.terminate(|_| {
                Ok(Exit::Jump(Edge {
                    target: target.target,
                    arguments,
                }))
            })
        })?;
        let continuation = self.arena.block(self.pending.id, &[])?;
        let otherwise = self.build_branch(None, |_| Ok(()))?;
        self.attach_conditional(condition, taken, otherwise, continuation)
    }

    /// Builds a branch that executes when the condition is true. A false condition
    /// skips it. The child has the same load, store, conditional and return methods.
    /// Return `Ok(())` without a terminal to continue after the branch.
    /// A closure error discards the branch and leaves the parent usable.
    /// Construction checks the branch even for a constant condition. Constant
    /// conditions are folded after the complete function body has been checked.
    ///
    /// Values depending on child reads, calls or joins can be consumed only in
    /// that child or its descendants. Pure expressions from parent values can be
    /// used on either path.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
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
    /// # Ok::<(), wasm86_compiler::BuildError>(())
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
    /// Both closures run during construction, including for constant conditions.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
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
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn if_else(
        &mut self,
        condition: impl Into<Val<I1>>,
        then_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
        else_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let condition = self.operand(condition)?;
        let branch = self.build_branch(None, then_build)?;
        let else_branch = self.build_branch(None, else_build)?;
        let condition = self.arena.normalize(condition)?;
        let continuation = self.arena.block(self.pending.id, &[])?;
        self.attach_conditional(condition, branch, else_branch, continuation)
    }

    /// Selects a typed result by executing one of two branches. Nonempty result
    /// arms must consume their builder with `yield_`, an outward `branch`,
    /// `return_`, `tail_call` or `trap`; at least one must yield
    /// to this conditional. Unit result arms may fall through.
    /// A yield supplies this conditional's result, while a return exits the function.
    /// A construction error discards both arms and leaves the parent usable.
    /// Both arms are checked even when the condition is constant.
    ///
    /// The selected value is visible in the parent. Other values depending on
    /// child reads, calls or joins remain confined to that child and its descendants.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
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
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn if_value<R: Results>(
        &mut self,
        condition: impl Into<Val<I1>>,
        then_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
        else_build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError> {
        let condition = self.operand(condition)?;
        let target = self.result_target::<R>()?;
        let branch = self.build_branch(Some(&target), then_build)?;
        let else_branch = self.build_branch(Some(&target), else_build)?;
        let condition = self.arena.normalize(condition)?;
        let outputs = self.join_outputs(&target, &[branch.entry, else_branch.entry])?;
        let values = crate::results::bind::<R>(self, &outputs);
        self.attach_conditional(condition, branch, else_branch, target.target)?;
        Ok(values)
    }
    fn attach_conditional(
        &mut self,
        condition: usize,
        taken: PendingBlock,
        otherwise: PendingBlock,
        join: BlockId,
    ) -> Result<(), BuildError> {
        self.connect_fallthrough(&taken, join)?;
        self.connect_fallthrough(&otherwise, join)?;
        self.arena.exit(
            self.pending.current,
            Exit::If {
                condition,
                taken: Edge {
                    target: taken.entry,
                    arguments: Vec::new(),
                },
                otherwise: Edge {
                    target: otherwise.entry,
                    arguments: Vec::new(),
                },
            },
        )?;
        self.pending.layout.push(Layout::If {
            branch: self.pending.current,
            taken: taken.layout,
            otherwise: otherwise.layout,
            join,
        });
        self.pending.current = join;
        Ok(())
    }
}
