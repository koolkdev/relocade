//! Typed construction and calls for generated functions.

use crate::{
    abi::{self, AbiType, CompilerValue, FunctionSignature, Values},
    results,
    value::ValueSource,
    Arguments, BlockBuilder, BuildError, Func, Program, Results,
};
use std::{fmt, marker::PhantomData};

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

impl<S> fmt::Debug for TypedFunc<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("TypedFunc")
            .field(&self.function)
            .finish()
    }
}

impl<S> TypedFunc<S> {
    pub(crate) fn new(function: Func) -> Self {
        Self {
            function,
            signature: PhantomData,
        }
    }
}

impl<S> From<TypedFunc<S>> for Func {
    fn from(function: TypedFunc<S>) -> Self {
        function.function
    }
}

impl Program {
    /// Registers a function with typed parameters and a reusable typed handle.
    /// Parameters arrive as compiler values, including named aggregate fields.
    /// Exporting is separate, so private helpers use the same construction path.
    /// The body is constructed during compilation if reachable from an export.
    ///
    /// The callback must complete the body with a return, tail call or trap.
    /// Capture owned inputs; construction errors are reported by [`Self::compile`].
    ///
    /// ```
    /// use relocade_compiler::Program;
    /// let mut program = Program::new();
    /// let increment = program.function::<fn(u32) -> u32>(move |body, (value,)| {
    ///     body.return_(value.add(1))
    /// });
    /// let run = program.function::<fn() -> u32>(move |mut body, ()| {
    ///     let value = increment.call(&mut body, 41)?;
    ///     body.return_(value)
    /// });
    /// program.export("run", run)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), relocade_compiler::BuildError>(())
    /// ```
    pub fn function<S>(
        &mut self,
        build: impl FnOnce(BlockBuilder<'_>, Values<S::Parameters>) -> Result<(), BuildError> + 'static,
    ) -> TypedFunc<S>
    where
        S: FunctionSignature<Parameters: CompilerValue, Return: AbiType<Logical: Results>>,
    {
        TypedFunc::new(self.raw_function(abi::signature::<S>(), move |body| {
            let mut index = 0;
            let parameters = results::bind_sources::<<S::Parameters as AbiType>::Logical>(|ty| {
                let value = body.arena.parameter(ty, index);
                index += 1;
                ValueSource::from_definition(body.arena.clone(), value)
            });
            build(body, S::Parameters::from_values(parameters))
        }))
    }
}

impl<S: FunctionSignature<Return: CompilerValue>> TypedFunc<S> {
    /// Calls this function and projects its results into the declared compiler type.
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
