use super::{AbiType, CompilerValue, Function, FunctionSignature, Values};
use crate::{BlockBuilder, BuildError, Program, TypedFunc};

impl<S: FunctionSignature<Parameters: CompilerValue, Return: AbiType<Logical: crate::Results>>>
    Function<'_, S>
{
    /// Defines and exports a function using this Rust signature.
    /// Parameters arrive as their symbolic values, including named aggregate fields.
    /// The export is registered immediately; compilation constructs the body.
    ///
    /// ```
    /// use relocade_compiler::{abi::Function, Program};
    /// const ADD: Function<fn(u32, u32) -> u32> = Function::new("example", "add");
    /// let mut program = Program::new();
    /// ADD.define(&mut program, move |body, (a, b)| body.return_(a.add(b)))?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), relocade_compiler::BuildError>(())
    /// ```
    pub fn define(
        self,
        program: &mut Program,
        build: impl FnOnce(BlockBuilder<'_>, Values<S::Parameters>) -> Result<(), BuildError> + 'static,
    ) -> Result<TypedFunc<S>, BuildError> {
        let function = program.function::<S>(build);
        self.export(program, function)?;
        Ok(function)
    }
}

impl<
        S: FunctionSignature<
            Parameters: AbiType<Logical: crate::Results>,
            Return: AbiType<Logical: crate::Results>,
        >,
    > Function<'_, S>
{
    /// Imports a function while retaining its Rust signature for generated calls.
    pub fn import(self, program: &mut Program) -> TypedFunc<S> {
        TypedFunc::new(program.import_function(self))
    }
}
