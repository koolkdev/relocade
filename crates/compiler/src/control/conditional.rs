//! Conditional execution and typed result selection.
use crate::{BuildError, FunctionBuilder, Operation, Results, Val, I1};

impl FunctionBuilder<'_> {
    /// Builds a branch that executes when the condition is true. A false condition
    /// skips it. The child has the same load, store, conditional and return methods.
    /// Returning `Ok(())` without a terminal lets execution continue after the branch.
    /// A closure error discards the branch and leaves the parent usable.
    ///
    /// Values depending on child reads, calls or joins can be consumed only in
    /// that child or its descendants. Pure expressions from parent values can be
    /// used on either path.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let function = program.declare(Signature {
    ///     parameters: vec![Type::I32], result: Some(Type::I32),
    /// });
    /// let mut body = program.define(function)?;
    /// let value = body.parameter::<I32>(0)?;
    /// body.if_(value.eq(0), |branch| branch.return_(7))?;
    /// body.return_(value.add(1))?;
    /// program.export("increment_or_seven", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn if_(
        &mut self,
        condition: impl Into<Val<I1>>,
        build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let condition = self.operand(condition)?;
        let branch = self.build_branch(None, build)?;
        let condition = self.arena.normalize(condition)?;
        self.region.operations.push(Operation::If {
            condition,
            branch,
            else_branch: None,
            outputs: Vec::new(),
        });
        Ok(())
    }

    /// Executes exactly one of two branches. Each branch may fall through,
    /// return from the function, tail-call or trap. A construction error discards both
    /// branches and leaves the parent usable. Child values follow `if_`'s scope rules.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let function = program.declare(Signature {
    ///     parameters: vec![Type::I32], result: Some(Type::I32),
    /// });
    /// let mut body = program.define(function)?;
    /// let value = body.parameter::<I32>(0)?;
    /// body.if_else(value.eq(0),
    ///     |branch| branch.return_(7),
    ///     |_branch| Ok(()),
    /// )?;
    /// body.return_(value.add(1))?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn if_else(
        &mut self,
        condition: impl Into<Val<I1>>,
        then_build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
        else_build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let condition = self.operand(condition)?;
        let branch = self.build_branch(None, then_build)?;
        let else_branch = self.build_branch(None, else_build)?;
        let condition = self.arena.normalize(condition)?;
        self.region.operations.push(Operation::If {
            condition,
            branch,
            else_branch: Some(else_branch),
            outputs: Vec::new(),
        });
        Ok(())
    }

    /// Selects a typed result by executing one of two branches. Nonempty result
    /// arms must consume their builder with `yield_`, an outward `branch`,
    /// `return_`, `return_void`, `tail_call` or `trap`; at least one must yield
    /// to this conditional. Unit result arms may fall through.
    /// A yield supplies this conditional's result, while a return exits the function.
    /// A construction error discards both arms and leaves the parent usable.
    ///
    /// The selected value is visible in the parent. Other values depending on
    /// child reads, calls or joins remain confined to that child and its descendants.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let function = program.declare(Signature {
    ///     parameters: vec![Type::I32], result: Some(Type::I32),
    /// });
    /// let mut body = program.define(function)?;
    /// let value = body.parameter::<I32>(0)?;
    /// let selected = body.if_value::<I32>(value.eq(0),
    ///     |arm| arm.yield_(7),
    ///     |arm| arm.yield_(value.add(1)),
    /// )?;
    /// body.return_(selected.add(2))?;
    /// program.export("choose_then_add", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn if_value<R: Results>(
        &mut self,
        condition: impl Into<Val<I1>>,
        then_build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
        else_build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError> {
        let condition = self.operand(condition)?;
        let target = self.result_target::<R>();
        let branch = self.build_branch(Some(&target), then_build)?;
        let else_branch = self.build_branch(Some(&target), else_build)?;
        let condition = self.arena.normalize(condition)?;
        let outputs = self.join_outputs(&target, [&branch, &else_branch])?;
        let values = super::results::bind::<R>(self, &outputs);
        self.region.operations.push(Operation::If {
            condition,
            branch,
            else_branch: Some(else_branch),
            outputs,
        });
        Ok(values)
    }
}
