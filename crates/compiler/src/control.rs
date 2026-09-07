use crate::{BuildError, FunctionBuilder, IntoOp, Operation, Terminal, I1};

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
            if let Operation::If { branch, .. } = operation {
                self.0.push(branch);
            }
        }
        Some(region)
    }
}

pub(super) enum Destination<'a> {
    Function,
    Branch(&'a mut Option<Region>),
}

impl FunctionBuilder<'_> {
    /// Builds a branch that executes when the condition is true. A false condition
    /// skips it. The child has the same load, store, conditional and return methods.
    /// Returning `Ok(())` without a terminal lets execution continue after the branch.
    /// A closure error discards the branch and leaves the parent usable.
    ///
    /// Values depending on child reads or calls can be consumed only in that child or its
    /// descendants. Pure expressions from parent values can be used on either path.
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
        let scope = self.arena.child_scope(self.region.id)?;
        let mut destination = None;
        build(FunctionBuilder {
            program: self.program,
            function: self.function,
            arena: self.arena.clone(),
            region: Region::new(scope),
            destination: Destination::Branch(&mut destination),
            fallthrough: true,
        })?;
        let branch = destination.ok_or(BuildError::IncompleteBranch)?;
        let condition = self.arena.normalize(condition)?;
        self.region
            .operations
            .push(Operation::If { condition, branch });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{BuildError, FunctionImport, MemoryImport, Program, Signature, Type, I32};
    use wasmparser::{Parser, Payload};

    #[test]
    fn a_failed_branch_discards_nested_effects_and_imports_without_closing_the_parent() {
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
            body.if_(true, |mut branch| {
                branch.store::<I32>(memory, 0, 9)?;
                branch.call::<I32>(target, &[])?;
                branch.if_(true, |inner| inner.tail_call(target, &[]))?;
                branch.parameter::<I32>(0)?;
                Ok(())
            }),
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
        assert_eq!(result.admit(&arena), Err(BuildError::BodyClosed));
        assert!(program.compile().is_ok());
    }
}
