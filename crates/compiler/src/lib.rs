//! Guest-independent construction of scalar WebAssembly functions.
//!
//! Declare each function, build its body, then choose its exports and compile:
//!
//! ```
//! use wasm86_compiler::{Program, Signature, Type, I32};
//!
//! let mut program = Program::new();
//! let increment = program.declare(Signature {
//!     parameters: vec![Type::I32],
//!     result: Type::I32,
//! });
//! let body = program.define(increment)?;
//! let value = body.parameter::<I32>(0)?;
//! body.return_(&value.add(1))?;
//! program.export("increment", increment)?;
//! let bytes = program.compile()?;
//! # Ok::<(), wasm86_compiler::BuildError>(())
//! ```
#![forbid(unsafe_code)]

mod arena;
mod emit;
mod locals;
mod memory;
mod module;
mod place;
mod types;
mod value;

use std::fmt;

use arena::ExpressionArena;
use memory::Location;
pub use memory::{Mem, MemoryImport, MemoryInt};
pub use types::{IntType, Type, I1, I16, I32, I64, I8};
pub use value::{IntLiteral, IntoOp, Val};

/// A function's parameter types and single return type.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Signature {
    pub parameters: Vec<Type>,
    pub result: Type,
}

/// A declared function.
///
/// Use only with the [`Program`] that declared it.
#[derive(Clone, Copy, Debug)]
pub struct Func(usize);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildError {
    UnknownFunction,
    UnknownMemory,
    AlreadyDefined,
    MissingBody,
    UnknownParameter,
    ForeignBody,
    BodyClosed,
    TypeMismatch { expected: Type, actual: Type },
    DuplicateExport,
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMemory => formatter.write_str("unknown memory declaration"),
            Self::UnknownFunction => formatter.write_str("unknown function declaration"),
            Self::AlreadyDefined => formatter.write_str("function already has a finished body"),
            Self::MissingBody => formatter.write_str("function has no finished body"),
            Self::UnknownParameter => formatter.write_str("unknown function parameter"),
            Self::ForeignBody => formatter.write_str("value belongs to another body"),
            Self::BodyClosed => formatter.write_str("function body is no longer open"),
            Self::TypeMismatch { expected, actual } => {
                write!(formatter, "expected {expected:?}, received {actual:?}")
            }
            Self::DuplicateExport => formatter.write_str("export name is already declared"),
        }
    }
}

impl std::error::Error for BuildError {}

/// The declarations, completed function bodies and exports of a module.
#[derive(Default)]
pub struct Program {
    functions: Vec<Declaration>,
    exports: Vec<(String, Func)>,
    memories: Vec<MemoryImport>,
}

struct Declaration {
    signature: Signature,
    body: Option<Body>,
}

struct Body {
    values: Vec<Value>,
    operations: Vec<Operation>,
    result: usize,
}

#[derive(Clone, Copy)]
enum Operation {
    Load(usize),
    Store { location: Location, value: usize },
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Value {
    ty: Type,
    kind: ValueKind,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum ValueKind {
    Constant(u64),
    Parameter(u32),
    Add(usize, usize),
    Load { location: Location, site: usize },
}

/// Builds one function body. Returning a value completes its definition.
///
/// Dropping this builder without returning leaves the function undefined.
/// A builder cannot be used after its program is consumed:
/// ```compile_fail
/// use wasm86_compiler::{Program, Signature, Type, I32};
/// let mut program = Program::new();
/// let function = program.declare(Signature { parameters: vec![], result: Type::I32 });
/// let body = program.define(function).unwrap();
/// let module = program.compile();
/// let result = body.constant::<I32>(0);
/// body.return_(&result).unwrap();
/// ```
pub struct FunctionBuilder<'p> {
    program: &'p mut Program,
    function: Func,
    arena: ExpressionArena,
    operations: Vec<Operation>,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares a function that must be defined before compilation.
    /// Functions are emitted in declaration order, including unexported functions.
    pub fn declare(&mut self, signature: Signature) -> Func {
        let function = Func(self.functions.len());
        self.functions.push(Declaration {
            signature,
            body: None,
        });
        function
    }

    /// Starts a body for a function that has no completed definition.
    pub fn define(&mut self, function: Func) -> Result<FunctionBuilder<'_>, BuildError> {
        let declaration = self
            .functions
            .get(function.0)
            .ok_or(BuildError::UnknownFunction)?;
        if declaration.body.is_some() {
            return Err(BuildError::AlreadyDefined);
        }
        Ok(FunctionBuilder {
            program: self,
            function,
            arena: ExpressionArena::new(),
            operations: Vec::new(),
        })
    }

    pub fn export(&mut self, name: &str, function: Func) -> Result<(), BuildError> {
        self.functions
            .get(function.0)
            .ok_or(BuildError::UnknownFunction)?;
        if self.exports.iter().any(|(existing, _)| existing == name) {
            return Err(BuildError::DuplicateExport);
        }
        self.exports.push((name.to_owned(), function));
        Ok(())
    }

    /// Encodes the module as WebAssembly bytes, consuming the program.
    /// Every declared function must have a completed body.
    pub fn compile(self) -> Result<Vec<u8>, BuildError> {
        if self
            .functions
            .iter()
            .any(|function| function.body.is_none())
        {
            return Err(BuildError::MissingBody);
        }
        Ok(module::encode(&self))
    }
}

impl FunctionBuilder<'_> {
    fn signature(&self) -> &Signature {
        &self.program.functions[self.function.0].signature
    }

    /// Selects a parameter by its zero-based position in the signature.
    /// The requested type must match the declared parameter type. Callers must
    /// supply narrow arguments zero-extended, as required by [`Type`].
    pub fn parameter<T: IntType>(&self, index: u32) -> Result<Val<T>, BuildError> {
        let actual = *self
            .signature()
            .parameters
            .get(index as usize)
            .ok_or(BuildError::UnknownParameter)?;
        if actual != T::TYPE {
            return Err(BuildError::TypeMismatch {
                expected: T::TYPE,
                actual,
            });
        }
        let value = self.arena.intern(Value {
            ty: actual,
            kind: ValueKind::Parameter(index),
        })?;
        Ok(Val::new(self.arena.clone(), Ok(value)))
    }

    /// Creates an integer constant in this body.
    pub fn constant<T: IntType>(&self, value: impl IntLiteral<T>) -> Val<T> {
        Val::constant(&self.arena, value)
    }

    /// Ends the generated function with this return value and saves its body,
    /// consuming the builder.
    /// On error, the unfinished body is discarded and the function remains undefined.
    pub fn return_<T: IntType>(mut self, result: &Val<T>) -> Result<(), BuildError> {
        let result = result.admit(&self.arena)?;
        let expected = self.signature().result;
        if T::TYPE != expected {
            return Err(BuildError::TypeMismatch {
                expected,
                actual: T::TYPE,
            });
        }
        let values = self.arena.take().ok_or(BuildError::BodyClosed)?;
        self.program.functions[self.function.0].body = Some(Body {
            values,
            operations: std::mem::take(&mut self.operations),
            result,
        });
        Ok(())
    }
}

impl Drop for FunctionBuilder<'_> {
    fn drop(&mut self) {
        // Returning successfully has already transferred the values. Every other exit
        // discards them and prevents retained handles from building more expressions.
        self.arena.take();
    }
}
