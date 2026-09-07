use crate::{
    Argument, BuildError, Declaration, Func, FunctionBuilder, FunctionKind, Program, Signature,
    Terminal,
};

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
    ///         result: Type::I64,
    ///     },
    /// });
    /// let block = program.declare(Signature {
    ///     parameters: vec![],
    ///     result: Type::I64,
    /// });
    /// let body = program.define(block)?;
    /// body.tail_call(dispatch, &[0x1004.into()])?;
    /// program.export("block", block)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn tail_call(mut self, target: Func, arguments: &[Argument]) -> Result<(), BuildError> {
        self.fallthrough = false;
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
        let expected = self.signature().result;
        if signature.result != expected {
            return Err(BuildError::TypeMismatch {
                expected,
                actual: signature.result,
            });
        }
        let mut values = Vec::with_capacity(arguments.len());
        for (argument, expected) in arguments.iter().zip(&signature.parameters) {
            let value = argument.admit(&self.arena, *expected)?;
            self.arena.require_visible(value, self.region.id)?;
            values.push(value);
        }
        // Validate every argument before creating shared results with upper bits cleared.
        for value in &mut values {
            *value = self.arena.normalize(*value)?;
        }
        self.complete(Terminal::TailCall {
            target,
            arguments: values,
        })
    }
}
