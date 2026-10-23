use super::{AbiType, CompilerValue, Function, FunctionSignature, Values};
use crate::{BlockBuilder, BuildError, Program, TypedFunc};

impl<S: FunctionSignature<Parameters: CompilerValue, Return: AbiType<Logical: crate::Results>>>
    Function<'_, S>
{
    /// Declares, builds and exports a function using this Rust signature.
    /// Parameters arrive as their symbolic values, including named aggregate fields.
    /// Construction has the rollback behavior of [`Program::function`].
    ///
    /// ```
    /// use relocade_compiler::{abi::Function, Program};
    /// const ADD: Function<fn(u32, u32) -> u32> = Function::new("example", "add");
    /// let mut program = Program::new();
    /// ADD.define(&mut program, |body, (a, b)| body.return_(a.add(b)))?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), relocade_compiler::BuildError>(())
    /// ```
    pub fn define(
        self,
        program: &mut Program,
        build: impl FnOnce(BlockBuilder<'_>, Values<S::Parameters>) -> Result<(), BuildError>,
    ) -> Result<TypedFunc<S>, BuildError> {
        program.function::<S>(|body, parameters| {
            body.program.export(self.name, body.function)?;
            build(body, parameters)
        })
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
