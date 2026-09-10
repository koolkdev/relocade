use crate::{
    control::Site, results, Argument, Body, BuildError, Declaration, Func, FunctionBuilder,
    FunctionKind, Operation, Program, Results, Signature, Terminal, Type,
};

pub(super) struct Invocation {
    pub(super) target: Func,
    pub(super) arguments: Vec<usize>,
}

impl Body {
    pub(super) fn call(&self, site: Site) -> (&Invocation, &[usize]) {
        let region = self
            .region
            .walk()
            .find(|region| region.id == site.region)
            .expect("a call result names an attached region");
        let Operation::Call {
            invocation,
            outputs,
        } = &region.operations[site.index]
        else {
            unreachable!("a call result names its invocation")
        };
        (invocation, outputs)
    }
}

/// An external function and its logical parameter and return types.
/// Narrow arguments are passed zero-extended. The imported function must return
/// narrow results zero-extended too, as required by [`crate::Type`].
pub struct FunctionImport {
    pub module: String,
    pub name: String,
    pub signature: Signature,
}

impl Program {
    /// Declares an imported function. Unused imports are omitted from the module.
    pub fn import_function(&mut self, import: FunctionImport) -> Func {
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

impl FunctionBuilder<'_> {
    /// Ends the generated function by returning the target function's result,
    /// saving the completed body and consuming the builder. The call does not
    /// retain this function's Wasm frame. Ordered stores run before the call.
    /// In a branch, only that branch is completed. An error when completing the
    /// outer builder discards the function body and leaves it undefined.
    ///
    /// Argument and result types must match logically, even when their Wasm
    /// representations coincide. The target may be imported or defined.
    ///
    /// ```
    /// use wasm86_compiler::{FunctionImport, Program, Signature, Type};
    ///
    /// let mut program = Program::new();
    /// let dispatch = program.import_function(FunctionImport {
    ///     module: "wasm86".into(),
    ///     name: "dispatch".into(),
    ///     signature: Signature {
    ///         parameters: vec![Type::I32],
    ///         results: vec![Type::I64],
    ///     },
    /// });
    /// let block = program.declare(Signature {
    ///     parameters: vec![],
    ///     results: vec![Type::I64],
    /// });
    /// let body = program.define(block)?;
    /// body.tail_call(dispatch, &[0x1004.into()])?;
    /// program.export("block", block)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn tail_call(mut self, target: Func, arguments: &[Argument]) -> Result<(), BuildError> {
        self.fallthrough = false;
        let invocation = self.resolve_call(target, arguments, &self.signature().results)?;
        self.complete(Terminal::TailCall(invocation))
    }

    /// Calls a function and returns its typed result shape, leaving this builder open.
    /// Components share one invocation and are visible in this branch and its
    /// descendants. `()` requests no results; tuples and arrays request several.
    ///
    /// Defined helpers without inferred writes or unknown effects can run later
    /// or disappear when unused. Possible traps in the call or its argument
    /// computations move or disappear with it. Read snapshots remain protected
    /// across overlapping writes. Calls that may write, call imports or reach
    /// unresolved recursion execute in authored order, even when their result is
    /// unused. Every declared result is evaluated when a call runs, including
    /// components its caller discards. Narrow results follow
    /// the same zero-extended calling convention as tail calls.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let increment = program.declare(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I32],
    /// });
    /// let helper = program.define(increment)?;
    /// let input = helper.parameter::<I32>(0)?;
    /// helper.return_(input.add(1))?;
    /// let function = program.declare(Signature { parameters: vec![], results: vec![Type::I32] });
    /// let mut body = program.define(function)?;
    /// let result = body.call::<I32>(increment, &[7.into()])?;
    /// body.return_(result.add(1))?;
    /// program.export("run", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn call<R: Results>(
        &mut self,
        target: Func,
        arguments: &[Argument],
    ) -> Result<R::Values, BuildError> {
        let types = results::types::<R>();
        let invocation = self.resolve_call(target, arguments, &types)?;
        let outputs = types
            .iter()
            .enumerate()
            .map(|(component, &ty)| self.arena.call_result(ty, self.site(), component))
            .collect::<Result<Vec<_>, _>>()?;
        let values = results::bind::<R>(self, &outputs);
        self.region.operations.push(Operation::Call {
            invocation,
            outputs,
        });
        Ok(values)
    }

    fn resolve_call(
        &self,
        target: Func,
        arguments: &[Argument],
        expected: &[Type],
    ) -> Result<Invocation, BuildError> {
        let signature = &self
            .program
            .functions
            .get(target.0)
            .ok_or(BuildError::UnknownFunction)?
            .signature;
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
            let value = argument.resolve(&self.arena, *expected, self.region.id)?;
            values.push(value);
        }
        // Validate every argument before creating shared results with upper bits cleared.
        for value in &mut values {
            *value = self.arena.normalize(*value)?;
        }
        Ok(Invocation {
            target,
            arguments: values,
        })
    }
}

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
        let discarded = program.define(function).unwrap();
        let foreign = discarded.value::<I1>(true).unwrap();
        drop(discarded);
        let mut body = program.define(function).unwrap();
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
        body.return_(7).unwrap();
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
        program.define(helper).unwrap().return_(7).unwrap();
        let function = program.declare(signature);
        let mut body = program.define(function).unwrap();
        let mut escaped = None;
        body.if_(false, |mut branch| {
            escaped = Some(branch.call::<I32>(helper, &[])?);
            Ok(())
        })
        .unwrap();
        let escaped = escaped.unwrap();
        for value in [escaped.add(1), escaped.and(0).add(1)] {
            assert_eq!(body.value(value).err(), Some(BuildError::OutOfScope));
        }
        body.return_(0).unwrap();
        assert!(program.compile().is_ok());
    }
}
