use crate::{
    Argument, BuildError, FunctionBuilder, IntType, IntoOp, Operation, Terminal, Type, Val, I1,
};

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Site {
    pub(super) region: usize,
    pub(super) index: usize,
}

pub(super) struct Region {
    pub(super) id: usize,
    pub(super) operations: Vec<Operation>,
    pub(super) terminal: Option<Terminal>,
}

impl Region {
    pub(super) fn new(id: usize) -> Self {
        Self {
            id,
            operations: Vec::new(),
            terminal: None,
        }
    }

    pub(super) fn walk(&self) -> Regions<'_> {
        Regions(vec![self])
    }
}

pub(super) struct Regions<'a>(Vec<&'a Region>);

impl<'a> Iterator for Regions<'a> {
    type Item = &'a Region;
    fn next(&mut self) -> Option<Self::Item> {
        let region = self.0.pop()?;
        for operation in region.operations.iter().rev() {
            if let Operation::If {
                branch,
                else_branch,
                ..
            } = operation
            {
                if let Some(other) = else_branch {
                    self.0.push(other);
                }
                self.0.push(branch);
            }
        }
        Some(region)
    }
}

pub(super) enum Destination<'a> {
    Function,
    Branch {
        region: &'a mut Option<Region>,
        result: Option<Type>,
    },
}

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
    ///     parameters: vec![Type::I32], result: Type::I32,
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
        condition: impl IntoOp<I1>,
        build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let condition = self.operand(condition)?;
        let branch = self.build_branch(None, build)?;
        let condition = self.arena.normalize(condition)?;
        self.region.operations.push(Operation::If {
            condition,
            branch,
            else_branch: None,
            output: None,
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
    ///     parameters: vec![Type::I32], result: Type::I32,
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
        condition: impl IntoOp<I1>,
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
            output: None,
        });
        Ok(())
    }

    /// Selects a value by executing one of two branches. Each arm must consume
    /// its builder with `yield_`, `return_`, `tail_call` or `trap`; at least one must yield.
    /// A yield supplies this conditional's value, while a return exits the function.
    /// A construction error discards both arms and leaves the parent usable.
    ///
    /// The selected value is visible in the parent. Other values depending on
    /// child reads, calls or joins remain confined to that child and its descendants.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let function = program.declare(Signature {
    ///     parameters: vec![Type::I32], result: Type::I32,
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
    pub fn if_value<T: IntType>(
        &mut self,
        condition: impl IntoOp<I1>,
        then_build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
        else_build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Val<T>, BuildError> {
        let condition = self.operand(condition)?;
        let branch = self.build_branch(Some(T::TYPE), then_build)?;
        let else_branch = self.build_branch(Some(T::TYPE), else_build)?;
        let results: Vec<_> = [&branch, &else_branch]
            .into_iter()
            .filter_map(|arm| match arm.terminal {
                Some(Terminal::Yield(value)) => Some(value),
                _ => None,
            })
            .collect();
        if results.is_empty() {
            return Err(BuildError::MissingBranchValue);
        }
        let condition = self.arena.normalize(condition)?;
        let output = self.arena.join_result(T::TYPE, self.site(), &results)?;
        self.region.operations.push(Operation::If {
            condition,
            branch,
            else_branch: Some(else_branch),
            output: Some(output),
        });
        Ok(Val::new(self.arena.clone(), Ok(output)))
    }

    /// Supplies this value-producing arm's result, consuming its builder.
    /// The enclosing `if_value` supplies a literal's type; typed values must match it.
    /// Execution then continues after that conditional, rather than returning
    /// from the function. Use only on the arm passed directly to `if_value`.
    /// An inner `if_` cannot yield on behalf of an enclosing value arm.
    pub fn yield_(mut self, value: impl Into<Argument>) -> Result<(), BuildError> {
        self.fallthrough = false;
        let Destination::Branch {
            result: Some(ty), ..
        } = self.destination
        else {
            return Err(BuildError::InvalidYield);
        };
        let value = self.argument(value, ty)?;
        self.complete(Terminal::Yield(value))
    }

    fn build_branch(
        &mut self,
        result: Option<Type>,
        build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Region, BuildError> {
        let scope = self.arena.child_scope(self.region.id)?;
        let mut destination = None;
        build(FunctionBuilder {
            program: self.program,
            function: self.function,
            arena: self.arena.clone(),
            region: Region::new(scope),
            destination: Destination::Branch {
                region: &mut destination,
                result,
            },
            fallthrough: true,
        })?;
        destination.ok_or(BuildError::IncompleteBranch)
    }
}

#[cfg(test)]
mod tests {
    use crate::{BuildError, FunctionImport, MemoryImport, Program, Signature, Type, I32, I8};
    use wasmparser::{Parser, Payload};

    #[test]
    fn a_failed_else_branch_discards_both_arms_without_closing_the_parent() {
        let mut program = Program::new();
        let memory = program.import_memory(MemoryImport {
            module: "test".into(),
            name: "memory".into(),
            minimum: 1,
            maximum: None,
        });
        let signature = Signature {
            parameters: vec![],
            result: Type::I32,
        };
        let target = program.import_function(FunctionImport {
            module: "test".into(),
            name: "target".into(),
            signature: signature.clone(),
        });
        let function = program.declare(signature);
        let mut body = program.define(function).unwrap();
        assert_eq!(
            body.if_else(
                true,
                |mut branch| {
                    branch.store::<I32>(memory, 0, 9)?;
                    branch.call::<I32>(target, &[])?;
                    branch.if_(true, |inner| inner.tail_call(target, &[]))?;
                    Ok(())
                },
                |branch| {
                    branch.parameter::<I32>(0)?;
                    Ok(())
                },
            ),
            Err(BuildError::UnknownParameter)
        );
        body.return_(7).unwrap();
        let bytes = program.compile().unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
    }

    #[test]
    fn a_swallowed_terminal_error_cannot_turn_into_branch_fallthrough() {
        let mut program = Program::new();
        let function = program.declare(Signature {
            parameters: vec![],
            result: Type::I32,
        });
        let mut body = program.define(function).unwrap();
        assert_eq!(
            body.if_(true, |branch| {
                assert_eq!(
                    branch.return_(true),
                    Err(BuildError::TypeMismatch {
                        expected: Type::I32,
                        actual: Type::I1,
                    })
                );
                Ok(())
            }),
            Err(BuildError::IncompleteBranch)
        );
        body.return_(7).unwrap();
        assert!(program.compile().is_ok());
    }

    #[test]
    fn completing_a_child_keeps_parent_values_open_until_the_function_completes() {
        let mut program = Program::new();
        let function = program.declare(Signature {
            parameters: vec![],
            result: Type::I32,
        });
        let mut body = program.define(function).unwrap();
        let value = body.value::<I32>(7).unwrap();
        body.if_(false, |branch| branch.return_(&value)).unwrap();
        let result = value.add(1);
        let arena = body.arena.clone();
        body.return_(&result).unwrap();
        assert_eq!(
            result.checked_expression(&arena),
            Err(BuildError::BodyClosed)
        );
        assert!(program.compile().is_ok());
    }

    #[test]
    fn a_failed_yield_discards_both_arms_without_retaining_their_imports() {
        let mut program = Program::new();
        let memory = program.import_memory(MemoryImport {
            module: "test".into(),
            name: "memory".into(),
            minimum: 1,
            maximum: None,
        });
        let signature = Signature {
            parameters: vec![],
            result: Type::I32,
        };
        let target = program.import_function(FunctionImport {
            module: "test".into(),
            name: "target".into(),
            signature: signature.clone(),
        });
        let function = program.declare(signature);
        let mut body = program.define(function).unwrap();
        let result = body.if_value::<I32>(
            true,
            |mut arm| {
                arm.store::<I32>(memory, 0, 9)?;
                arm.yield_(7)
            },
            |mut arm| {
                arm.call::<I32>(target, &[])?;
                let byte = arm.value::<I8>(1)?;
                assert_eq!(
                    arm.yield_(byte),
                    Err(BuildError::TypeMismatch {
                        expected: Type::I32,
                        actual: Type::I8,
                    })
                );
                Ok(())
            },
        );
        assert_eq!(result.err(), Some(BuildError::IncompleteBranch));
        body.return_(7).unwrap();
        let bytes = program.compile().unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
    }

    #[test]
    fn yielding_a_nested_join_exposes_only_the_new_parent_result() {
        let mut program = Program::new();
        let function = program.declare(Signature {
            parameters: vec![],
            result: Type::I32,
        });
        let mut body = program.define(function).unwrap();
        let mut escaped = None;
        let selected = body
            .if_value::<I32>(
                true,
                |mut arm| {
                    let nested = arm.if_value::<I32>(
                        false,
                        |inner| inner.yield_(1),
                        |inner| inner.yield_(2),
                    )?;
                    escaped = Some(nested.clone());
                    arm.yield_(nested)
                },
                |arm| arm.yield_(3),
            )
            .unwrap();
        assert_eq!(
            body.value(escaped.unwrap().add(1)).err(),
            Some(BuildError::OutOfScope)
        );
        let result = selected.add(1);
        let arena = body.arena.clone();
        body.return_(&result).unwrap();
        assert_eq!(
            result.checked_expression(&arena),
            Err(BuildError::BodyClosed)
        );
        assert!(program.compile().is_ok());
    }
}
