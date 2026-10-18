//! Shared declarations for generated imports, exports and host bindings.

use std::marker::PhantomData;

use crate::{
    results, BuildError, Func, FunctionImport, MemoryImport, Program, Results, Signature,
    TableImport, ValueType,
};

/// A named function whose signature uses the compiler's logical value types.
/// Embeddings use the same declaration to bind an import or resolve an export.
#[derive(Clone, Copy, Debug)]
pub struct Function<'a, S> {
    pub module: &'a str,
    pub name: &'a str,
    signature: PhantomData<S>,
}

impl<'a, S> Function<'a, S> {
    pub const fn new(module: &'a str, name: &'a str) -> Self {
        Self {
            module,
            name,
            signature: PhantomData,
        }
    }
}

impl<S: FunctionSignature> Function<'_, S> {
    pub fn signature(self) -> Signature {
        S::signature()
    }

    /// Exports a defined or imported function after checking its logical signature.
    /// The module name is used when importing; exports use only the field name.
    pub fn export(self, program: &mut Program, function: Func) -> Result<(), BuildError> {
        let actual = &program
            .functions
            .get(function.0)
            .ok_or(BuildError::UnknownFunction)?
            .signature;
        let expected = S::signature();
        if actual.parameters.len() != expected.parameters.len() {
            return Err(BuildError::ArgumentCount {
                expected: expected.parameters.len(),
                actual: actual.parameters.len(),
            });
        }
        results::check_count(expected.results.len(), actual.results.len())?;
        for (&expected, &actual) in expected
            .parameters
            .iter()
            .chain(&expected.results)
            .zip(actual.parameters.iter().chain(&actual.results))
        {
            if expected != actual {
                return Err(BuildError::TypeMismatch { expected, actual });
            }
        }
        program.export(self.name, function)
    }
}

impl<S: FunctionSignature> From<Function<'_, S>> for FunctionImport {
    fn from(function: Function<'_, S>) -> Self {
        Self {
            module: function.module.into(),
            name: function.name.into(),
            signature: S::signature(),
        }
    }
}

/// A function-pointer shape made from logical parameters and a [`Results`] shape.
/// For example, `fn(I16, I32) -> I64` keeps the first parameter narrow even though
/// both parameters use Wasm's i32 carrier. Tuples and arrays support multiple results.
pub trait FunctionSignature {
    fn signature() -> Signature;
}

macro_rules! signatures {
    ($($parameter:ident),*) => {
        impl<R: Results, $($parameter: ValueType),*> FunctionSignature for fn($($parameter),*) -> R {
            fn signature() -> Signature {
                Signature { parameters: vec![$($parameter::TYPE),*], results: results::types::<R>() }
            }
        }
    };
}

signatures!();
signatures!(A);
signatures!(A, B);
signatures!(A, B, C);
signatures!(A, B, C, D);

/// A named memory and the limits required by generated modules, in 64-KiB pages.
/// The embedding may allocate a larger memory within these limits.
#[derive(Clone, Copy, Debug)]
pub struct Memory {
    pub module: &'static str,
    pub name: &'static str,
    pub minimum: u32,
    pub maximum: Option<u32>,
    pub shared: bool,
}

impl Memory {
    /// Declares an unshared memory with no maximum.
    pub const fn new(module: &'static str, name: &'static str, minimum: u32) -> Self {
        Self {
            module,
            name,
            minimum,
            maximum: None,
            shared: false,
        }
    }
}

impl From<Memory> for MemoryImport {
    fn from(memory: Memory) -> Self {
        Self {
            module: memory.module.into(),
            name: memory.name.into(),
            minimum: memory.minimum,
            maximum: memory.maximum,
            shared: memory.shared,
        }
    }
}

/// A named table whose callable entries have the same logical signature.
#[derive(Clone, Copy, Debug)]
pub struct Table<S> {
    pub module: &'static str,
    pub name: &'static str,
    pub minimum: u32,
    pub maximum: Option<u32>,
    signature: PhantomData<S>,
}

impl<S> Table<S> {
    pub const fn new(module: &'static str, name: &'static str, minimum: u32) -> Self {
        Self {
            module,
            name,
            minimum,
            maximum: None,
            signature: PhantomData,
        }
    }
}

impl<S: FunctionSignature> From<Table<S>> for TableImport {
    fn from(table: Table<S>) -> Self {
        Self {
            module: table.module.into(),
            name: table.name.into(),
            minimum: table.minimum,
            maximum: table.maximum,
            signature: S::signature(),
        }
    }
}
