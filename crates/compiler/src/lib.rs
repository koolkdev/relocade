//! Guest-independent construction of typed WebAssembly functions.
//!
//! [`Program`] owns function and memory declarations. [`Val`] constructs typed
//! expressions; [`Type`] specifies logical widths and their Wasm calling convention.
//! [`BlockBuilder`] adds memory effects, branches and loops, using [`Results`]
//! for typed result shapes. [`Mem`] and [`MemoryImport`] describe external memory.
//! Their API documentation covers value visibility, effect ordering and examples.
//!
//! Define functions, choose their exports, then construct and compile reachable bodies:
//!
//! ```
//! use relocade_compiler::Program;
//!
//! let mut program = Program::new();
//! let increment =
//!     program.function::<fn(u32) -> u32>(move |body, (value,)| body.return_(value.add(1)));
//! program.export("increment", increment)?;
//! let bytes = program.compile()?;
//! # Ok::<(), relocade_compiler::BuildError>(())
//! ```
#![forbid(unsafe_code)]

pub mod abi;
mod arena;
mod bitwise;
mod body;
mod call;
mod control;
mod emit;
mod expression;
mod floating;
mod function;
mod integer;
mod literal;
mod memory;
mod module;
mod place;
mod results;
mod types;
mod value;

use std::fmt;

use body::FunctionGraph;
pub use call::{FunctionImport, Table, TableImport};
pub use control::{Label, LoopLabels};
use expression::Expression;
pub use function::{BlockBuilder, TypedFunc};
pub use memory::{AtomicAccess, Mem, MemoryImport, MemoryInt, MemoryType};
pub use results::{Arguments, Results};
pub use types::{
    AtLeast, BitwiseType, DoubleWidth, IntType, Type, ValueType, F64, I1, I16, I32, I64, I8, V128,
    V64,
};
pub use value::{
    Argument, F32x4, F64x2, SaturatingLane, Signed, Unsigned, Val, VectorLane, VectorType,
};

/// A function's ordered logical parameter and result types.
/// Use with [`Program::raw_function`] when generation assembles the type lists.
/// These are logical types, including `I1` and `I16`, rather than lowered Wasm types.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Signature {
    pub parameters: Vec<Type>,
    pub results: Vec<Type>,
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
    UnknownTable,
    ImportedFunction,
    ArgumentCount { expected: usize, actual: usize },
    AlreadyDefined,
    MissingBody,
    UnknownParameter,
    ForeignBody,
    BodyClosed,
    OutOfScope,
    InvalidYield,
    MissingBranchValue,
    DuplicateSwitchCase { key: u32 },
    SwitchCaseOutOfRange { key: u32, selector: Type },
    TypeMismatch { expected: Type, actual: Type },
    ResultCount { expected: usize, actual: usize },
    DuplicateExport,
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImportedFunction => {
                formatter.write_str("an imported function cannot have a body")
            }
            Self::ArgumentCount { expected, actual } => {
                write!(
                    formatter,
                    "expected {expected} arguments, received {actual}"
                )
            }
            Self::UnknownTable => formatter.write_str("unknown function table declaration"),
            Self::UnknownMemory => formatter.write_str("unknown memory declaration"),
            Self::UnknownFunction => formatter.write_str("unknown function declaration"),
            Self::AlreadyDefined => formatter.write_str("function already has a definition"),
            Self::MissingBody => formatter.write_str("function has no finished body"),
            Self::UnknownParameter => formatter.write_str("unknown function parameter"),
            Self::ForeignBody => formatter.write_str("value or label belongs to another body"),
            Self::OutOfScope => {
                formatter.write_str("value or label is not available in this scope")
            }
            Self::InvalidYield => {
                formatter.write_str("yield requires a direct result arm or block")
            }
            Self::MissingBranchValue => {
                formatter.write_str("no branch supplies the required result")
            }
            Self::DuplicateSwitchCase { key } => {
                write!(formatter, "switch case {key} appears more than once")
            }
            Self::SwitchCaseOutOfRange { key, selector } => {
                write!(formatter, "switch case {key} does not fit {selector:?}")
            }
            Self::BodyClosed => formatter.write_str("function body is no longer open"),
            Self::TypeMismatch { expected, actual } => {
                write!(formatter, "expected {expected:?}, received {actual:?}")
            }
            Self::ResultCount { expected, actual } => {
                write!(formatter, "expected {expected} results, received {actual}")
            }
            Self::DuplicateExport => formatter.write_str("export name is already declared"),
        }
    }
}

impl std::error::Error for BuildError {}

/// Optional WebAssembly extensions the destination engine supports.
/// These capabilities affect lowering, independently of logical value types.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WasmFeatures {
    /// Allow native `i64.mul_wide_s` and `i64.mul_wide_u` instructions.
    /// Disabled by default; enable only for engines supporting wide arithmetic.
    pub wide_arithmetic: bool,
}

/// The function definitions, imports and exports of a module.
#[derive(Default)]
pub struct Program {
    functions: Vec<Declaration>,
    exports: Vec<(String, Func)>,
    memories: Vec<MemoryImport>,
    tables: Vec<TableImport>,
    features: WasmFeatures,
}

struct Declaration {
    signature: Signature,
    kind: FunctionKind,
}

enum FunctionKind {
    Declared,
    Pending(function::BuildBody),
    Building,
    Built(FunctionGraph),
    Imported { module: String, name: String },
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs a program for a destination with the specified capabilities.
    /// `new()` uses portable lowering for optional extensions.
    pub fn with_features(features: WasmFeatures) -> Self {
        Self {
            features,
            ..Self::default()
        }
    }

    /// Declares a function for forward references. A reachable declaration must
    /// have a builder registered with [`Self::define`] before it is constructed.
    pub fn declare(&mut self, signature: Signature) -> Func {
        let function = Func(self.functions.len());
        self.functions.push(Declaration {
            signature,
            kind: FunctionKind::Declared,
        });
        function
    }

    pub fn export(&mut self, name: &str, function: impl Into<Func>) -> Result<(), BuildError> {
        let function = function.into();
        self.functions
            .get(function.0)
            .ok_or(BuildError::UnknownFunction)?;
        if self.exports.iter().any(|(existing, _)| existing == name) {
            return Err(BuildError::DuplicateExport);
        }
        self.exports.push((name.to_owned(), function));
        Ok(())
    }

    /// Constructs IR starting from exports, then infers effects, optimizes and
    /// emits WebAssembly. Each reachable body builder runs once. Unused definitions
    /// and declarations are ignored. Calls removed by optimization may still require
    /// constructing their callee to determine its effects.
    pub fn compile(mut self) -> Result<Vec<u8>, BuildError> {
        self.build_reachable_functions()?;
        Ok(module::encode(self))
    }
}
