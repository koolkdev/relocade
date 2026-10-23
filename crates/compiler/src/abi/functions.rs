use super::{AbiType, CompilerValue, Function, FunctionSignature, Values};
use crate::{results, value::ValueSource, Arguments, BlockBuilder, BuildError, Func, Program};
use std::marker::PhantomData;

/// A generated function retaining its Rust signature for subsequent calls.
/// Use it only with the program that declared it.
pub struct TypedFunc<S> {
    function: Func,
    signature: PhantomData<S>,
}

impl<S> Copy for TypedFunc<S> {}

impl<S> Clone for TypedFunc<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> TypedFunc<S> {
    fn new(function: Func) -> Self {
        Self {
            function,
            signature: PhantomData,
        }
    }

    /// Obtains the same function for APIs that use an explicit logical signature.
    pub fn untyped(self) -> Func {
        self.function
    }
}

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
        program
            .function(super::signature::<S>(), |body| {
                body.program.export(self.name, body.function)?;
                let mut index = 0;
                let parameters =
                    results::bind_sources::<<S::Parameters as AbiType>::Logical>(|ty| {
                        let value = body.arena.parameter(ty, index);
                        index += 1;
                        ValueSource::from_definition(body.arena.clone(), value)
                    });
                build(body, S::Parameters::from_values(parameters))
            })
            .map(TypedFunc::new)
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
impl<S: FunctionSignature<Return: CompilerValue>> TypedFunc<S> {
    /// Calls this function and projects its results into the declared symbolic type.
    /// Arguments retain the compiler's signature-directed literal conversion.
    pub fn call(
        self,
        body: &mut BlockBuilder<'_>,
        arguments: impl Into<Arguments>,
    ) -> Result<Values<S::Return>, BuildError> {
        let values =
            body.call::<<S::Return as AbiType>::Logical>(self.function, &arguments.into().0)?;
        Ok(S::Return::from_values(values))
    }
}

impl<S> TypedFunc<S> {
    /// Returns this function's results without retaining the caller's Wasm frame.
    pub fn tail_call(
        self,
        body: BlockBuilder<'_>,
        arguments: impl Into<Arguments>,
    ) -> Result<(), BuildError> {
        body.tail_call(self.function, &arguments.into().0)
    }
}
