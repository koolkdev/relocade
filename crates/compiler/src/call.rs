use crate::{
    body::{Exit, Operation},
    results, Argument, BlockBuilder, BuildError, Declaration, Func, FunctionKind, Program, Results,
    Signature, Type, Val, I32,
};

/// An external function and its logical parameter and return types.
/// Narrow arguments are passed zero-extended. The imported function must return
/// narrow results zero-extended too, as required by [`crate::Type`].
pub struct FunctionImport {
    pub module: String,
    pub name: String,
    pub signature: Signature,
}

/// A table of functions sharing a logical calling convention.
/// Use only with the program that declared it.
#[derive(Clone, Copy, Debug)]
pub struct Table(pub(super) usize);

/// An imported Wasm function table. The embedding must populate its callable
/// slots with functions satisfying this logical signature, including narrow types.
pub struct TableImport {
    pub module: String,
    pub name: String,
    pub minimum: u32,
    pub maximum: Option<u32>,
    pub signature: Signature,
}

impl Program {
    /// Declares a function table. Unused tables are omitted from the module.
    /// Panics when the maximum is smaller than the minimum.
    pub fn import_table(&mut self, import: impl Into<TableImport>) -> Table {
        let import = import.into();
        assert!(import
            .maximum
            .is_none_or(|maximum| maximum >= import.minimum));
        let table = Table(self.tables.len());
        self.tables.push(import);
        table
    }

    /// Declares an imported function. Unused imports are omitted from the module.
    pub fn import_function(&mut self, import: impl Into<FunctionImport>) -> Func {
        let import = import.into();
        let function = Func(self.functions.len());
        self.functions.push(Declaration {
            signature: import.signature,
            building: false,
            kind: FunctionKind::Imported {
                module: import.module,
                name: import.name,
            },
        });
        function
    }
}

impl BlockBuilder<'_> {
    /// Ends this execution path by returning the target function's result,
    /// consuming the builder. The call does not
    /// retain this function's Wasm frame. Ordered stores run before the call.
    /// The parent attaches the block after the callback succeeds. A callback
    /// error discards the completed block too.
    ///
    /// Argument and result types must match logically, even when their Wasm
    /// representations coincide. The target may be imported or defined.
    ///
    /// ```
    /// use relocade_compiler::{FunctionImport, Program, Signature, Type};
    ///
    /// let mut program = Program::new();
    /// let dispatch = program.import_function(FunctionImport {
    ///     module: "relocade".into(),
    ///     name: "dispatch".into(),
    ///     signature: Signature {
    ///         parameters: vec![],
    ///         results: vec![Type::I64],
    ///     },
    /// });
    /// let block = program.function(Signature {
    ///     parameters: vec![],
    ///     results: vec![Type::I64],
    /// }, |body| {
    ///     body.tail_call(dispatch, &[])
    /// })?;
    /// program.export("block", block)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), relocade_compiler::BuildError>(())
    /// ```
    pub fn tail_call(self, target: Func, arguments: &[Argument]) -> Result<(), BuildError> {
        self.terminate(|body| {
            let arguments = body.resolve_call(target, arguments, &body.signature().results)?;
            Ok(Exit::TailCall { target, arguments })
        })
    }

    /// Tail-calls a function selected from an imported table. The table's logical
    /// signature is checked just like a direct call. Stores precede the call;
    /// an invalid slot or mismatched Wasm signature traps in the destination engine.
    pub fn tail_call_indirect(
        self,
        table: Table,
        slot: impl Into<Val<I32>>,
        arguments: &[Argument],
    ) -> Result<(), BuildError> {
        self.terminate(|body| {
            let signature = &body
                .program
                .tables
                .get(table.0)
                .ok_or(BuildError::UnknownTable)?
                .signature;
            let arguments =
                body.resolve_arguments(signature, arguments, &body.signature().results)?;
            let slot = body.operand(slot)?;
            Ok(Exit::TailCallIndirect {
                table,
                slot,
                arguments,
            })
        })
    }

    /// Calls a function and returns its typed result shape, leaving this builder open.
    /// Components share one invocation and are visible in this branch and its
    /// descendants. `()` requests no results; tuples and arrays request several.
    ///
    /// Defined helpers without inferred writes, synchronization or unknown effects
    /// can run later or disappear when unused. Possible traps in the call or its
    /// argument computations move or disappear with it. Read snapshots remain
    /// protected across overlapping writes and explicit atomic effects. A shared
    /// result may keep its invocation before the paths that consume it. Calls that
    /// may write, synchronize, call imports or reach unresolved recursion execute
    /// in authored order, even when their result is unused. Every declared result
    /// is evaluated when a call runs, including components its caller discards.
    /// Narrow results follow the same zero-extended calling convention as tail calls.
    ///
    /// ```
    /// use relocade_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let increment = program.function(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I32],
    /// }, |helper| {
    ///     let input = helper.parameter::<I32>(0)?;
    ///     helper.return_(input.add(1))
    /// })?;
    /// let function = program.function(Signature { parameters: vec![], results: vec![Type::I32] }, |mut body| {
    ///     let result = body.call::<I32>(increment, &[7.into()])?;
    ///     body.return_(result.add(1))
    /// })?;
    /// program.export("run", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), relocade_compiler::BuildError>(())
    /// ```
    pub fn call<R: Results>(
        &mut self,
        target: Func,
        arguments: &[Argument],
    ) -> Result<R::Values, BuildError> {
        let types = results::types::<R>();
        let arguments = self.resolve_call(target, arguments, &types)?;
        let outputs = self.execute(Operation::call(target, arguments), &types)?;
        let values = results::bind::<R>(self, &outputs);
        Ok(values)
    }

    fn resolve_call(
        &self,
        target: Func,
        arguments: &[Argument],
        expected: &[Type],
    ) -> Result<Vec<usize>, BuildError> {
        let signature = &self
            .program
            .functions
            .get(target.0)
            .ok_or(BuildError::UnknownFunction)?
            .signature;
        self.resolve_arguments(signature, arguments, expected)
    }

    fn resolve_arguments(
        &self,
        signature: &Signature,
        arguments: &[Argument],
        expected: &[Type],
    ) -> Result<Vec<usize>, BuildError> {
        if arguments.len() != signature.parameters.len() {
            return Err(BuildError::ArgumentCount {
                expected: signature.parameters.len(),
                actual: arguments.len(),
            });
        }
        results::check_count(expected.len(), signature.results.len())?;
        for (&expected, &actual) in expected.iter().zip(&signature.results) {
            if expected != actual {
                return Err(BuildError::TypeMismatch { expected, actual });
            }
        }
        let mut values = Vec::with_capacity(arguments.len());
        for (argument, expected) in arguments.iter().zip(&signature.parameters) {
            let value = argument.resolve(&self.arena, *expected, self.pending.id)?;
            values.push(value);
        }
        // Validate every argument before creating shared results with upper bits cleared.
        for value in &mut values {
            *value = self.arena.normalize(*value)?;
        }
        Ok(values)
    }
}

#[cfg(test)]
#[path = "call/tables.rs"]
mod tables;

#[cfg(test)]
mod tests {
    use crate::{BuildError, FunctionImport, Program, Signature, Type, I1, I32, I8};
    use wasmparser::{Parser, Payload};

    #[test]
    fn a_failed_call_does_not_retain_an_import_or_close_the_body() {
        let mut program = Program::new();
        let target = program.import_function(FunctionImport {
            module: "test".into(),
            name: "target".into(),
            signature: Signature {
                parameters: vec![Type::I1],
                results: vec![Type::I8],
            },
        });
        let function = program.declare(Signature {
            parameters: vec![],
            results: vec![Type::I32],
        });
        let mut foreign = None;
        assert_eq!(
            program.define(function, |discarded| {
                foreign = Some(discarded.value::<I1>(true).unwrap());
                Ok(())
            }),
            Err(BuildError::MissingBody)
        );
        let foreign = foreign.unwrap();
        program
            .define(function, |mut body| {
                assert_eq!(
                    body.call::<I1>(target, &[true.into()]).err(),
                    Some(BuildError::TypeMismatch {
                        expected: Type::I1,
                        actual: Type::I8
                    })
                );
                let byte = body.value::<I8>(1).unwrap();
                assert_eq!(
                    body.call::<I8>(target, &[byte.into()]).err(),
                    Some(BuildError::TypeMismatch {
                        expected: Type::I1,
                        actual: Type::I8
                    })
                );
                assert_eq!(
                    body.call::<I8>(target, &[foreign.into()]).err(),
                    Some(BuildError::ForeignBody)
                );
                body.return_(7)
            })
            .unwrap();
        let bytes = program.compile().unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
    }

    #[test]
    fn a_call_result_cannot_escape_its_branch_through_arithmetic() {
        let mut program = Program::new();
        let signature = Signature {
            parameters: vec![],
            results: vec![Type::I32],
        };
        let helper = program.declare(signature.clone());
        program.define(helper, |body| body.return_(7)).unwrap();
        let function = program.declare(signature);
        program
            .define(function, |mut body| {
                let mut escaped = None;
                body.if_(true, |mut branch| {
                    escaped = Some(branch.call::<I32>(helper, &[])?);
                    Ok(())
                })
                .unwrap();
                let escaped = escaped.unwrap();
                for value in [escaped.add(1), escaped.and(0).add(1)] {
                    assert_eq!(body.value(value).err(), Some(BuildError::OutOfScope));
                }
                body.return_(0)
            })
            .unwrap();
        assert!(program.compile().is_ok());
    }
}
