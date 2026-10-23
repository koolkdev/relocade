//! Shared declarations for generated imports, exports and host bindings.

use std::marker::PhantomData;

use crate::{
    results, BuildError, Func, FunctionImport, MemoryImport, Program, Signature, TableImport,
};

mod functions;
#[doc(hidden)]
pub mod slots;
mod types;
mod values;
/// Derives named compiler values for a type with a portable ABI.
///
/// Use `#[derive(relocade_wasm_bindings::WasmAbi, CompilerValue)]` on the Rust definition.
/// Structs gain `NameValues` with symbolic fields. Enums gain `NameValues` with
/// snake_case constructors and `match_`, whose callback receives `NameCase` for
/// an exhaustive Rust match. Fields accept their symbolic values; enum constructors
/// also accept values convertible into them, including scalar literals.
///
/// The ABI determines logical widths and shared enum slots. This derive projects
/// compiler IR into that layout; it does not choose a second representation.
/// Generic structs may be instantiated with compiler-only fields such as vectors
/// or arbitrary-length arrays, without Rust codecs. A concrete compiler-only
/// struct can implement `AbiType` manually and derive `CompilerValue`.
/// Generated bodies retain the compiler's logical checks:
/// distinct Rust types with identical logical fields have the same IR signature.
///
/// ```
/// use relocade_wasm_bindings::WasmAbi;
/// use relocade_compiler::{abi::{Function, CompilerValue}, Program};
/// #[derive(WasmAbi, CompilerValue)]
/// struct Position { x: u32, y: u32 }
/// #[derive(WasmAbi, CompilerValue)]
/// enum Reply { Missing, Found(Position) }
/// let mut program = Program::new();
/// let locate = Function::<fn(u32, u32) -> Reply>::new("example", "locate")
///     .define(&mut program, move |body, (x, y)| {
///         body.return_(ReplyValues::found(PositionValues { x, y }))
///     })?;
/// Function::<fn() -> u32>::new("example", "run")
///     .define(&mut program, move |mut body, ()| {
///         let reply = locate.call(&mut body, (20, 22))?;
///         reply.match_(&mut body, |arm, case| match case {
///             ReplyCase::Missing => arm.return_(0),
///             ReplyCase::Found(position) => arm.return_(position.x.add(position.y)),
///         })?;
///         body.trap()
///     })?;
/// let bytes = program.compile()?;
/// # Ok::<(), relocade_compiler::BuildError>(())
/// ```
pub use relocade_derive::CompilerValue;
pub use relocade_wasm_bindings::{AbiType, AbiValue, Carrier, FunctionSignature, Value, WasmRepr};
pub use types::{CompilerValue, Values};
pub use values::{constant, WasmValues};

/// A named function whose Rust signature determines its logical value types.
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

impl<
        S: FunctionSignature<
            Parameters: AbiType<Logical: crate::Results>,
            Return: AbiType<Logical: crate::Results>,
        >,
    > Function<'_, S>
{
    pub fn signature(self) -> Signature {
        signature::<S>()
    }

    /// Exports a defined or imported function after checking its logical signature.
    /// The module name is used when importing; exports use only the field name.
    pub fn export(
        self,
        program: &mut Program,
        function: impl Into<Func>,
    ) -> Result<(), BuildError> {
        let function = function.into();
        let actual = &program
            .functions
            .get(function.0)
            .ok_or(BuildError::UnknownFunction)?
            .signature;
        let expected = signature::<S>();
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

impl<
        S: FunctionSignature<
            Parameters: AbiType<Logical: crate::Results>,
            Return: AbiType<Logical: crate::Results>,
        >,
    > From<Function<'_, S>> for FunctionImport
{
    fn from(function: Function<'_, S>) -> Self {
        Self {
            module: function.module.into(),
            name: function.name.into(),
            signature: signature::<S>(),
        }
    }
}

pub(crate) fn signature<
    S: FunctionSignature<
        Parameters: AbiType<Logical: crate::Results>,
        Return: AbiType<Logical: crate::Results>,
    >,
>() -> Signature {
    Signature {
        parameters: results::types::<<S::Parameters as AbiType>::Logical>(),
        results: results::types::<<S::Return as AbiType>::Logical>(),
    }
}

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

impl<
        S: FunctionSignature<
            Parameters: AbiType<Logical: crate::Results>,
            Return: AbiType<Logical: crate::Results>,
        >,
    > From<Table<S>> for TableImport
{
    fn from(table: Table<S>) -> Self {
        Self {
            module: table.module.into(),
            name: table.name.into(),
            minimum: table.minimum,
            maximum: table.maximum,
            signature: signature::<S>(),
        }
    }
}
