//! Guest-independent construction of scalar WebAssembly functions.
//!
//! Build functions, choose their exports, then compile the module:
//!
//! ```
//! use wasm86_compiler::{Program, Signature, Type, I32};
//!
//! let mut program = Program::new();
//! let increment = program.function(Signature {
//!     parameters: vec![Type::I32],
//!     result: Type::I32,
//! }, |body| {
//!     let value = body.parameter::<I32>(0)?;
//!     body.return_(value.add(1))
//! })?;
//! program.export("increment", increment)?;
//! let bytes = program.compile()?;
//! # Ok::<(), wasm86_compiler::BuildError>(())
//! ```
#![forbid(unsafe_code)]

mod arena;
mod call;
mod control;
mod effects;
mod emit;
mod integer;
mod locals;
mod memory;
mod module;
mod place;
mod types;
mod value;

use std::fmt;

use arena::ExpressionArena;
pub use call::FunctionImport;
use call::Invocation;
use control::{Destination, Region, Site};
use integer::{BinaryOp, CompareOp, ShiftOp};
use memory::Location;
pub use memory::{Mem, MemoryImport, MemoryInt};
pub use types::{AtLeast, IntType, Type, I1, I16, I32, I64, I8};
pub use value::{Argument, IntoOp, Signed, Unsigned, Val};

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
    ImportedFunction,
    ArgumentCount { expected: usize, actual: usize },
    AlreadyDefined,
    MissingBody,
    UnknownParameter,
    ForeignBody,
    BodyClosed,
    OutOfScope,
    IncompleteBranch,
    InvalidYield,
    MissingBranchValue,
    TypeMismatch { expected: Type, actual: Type },
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
            Self::UnknownMemory => formatter.write_str("unknown memory declaration"),
            Self::UnknownFunction => formatter.write_str("unknown function declaration"),
            Self::AlreadyDefined => formatter.write_str("function already has a finished body"),
            Self::MissingBody => formatter.write_str("function has no finished body"),
            Self::UnknownParameter => formatter.write_str("unknown function parameter"),
            Self::ForeignBody => formatter.write_str("value belongs to another body"),
            Self::OutOfScope => formatter.write_str("value is not available in this branch"),
            Self::IncompleteBranch => formatter.write_str("branch termination did not complete"),
            Self::InvalidYield => {
                formatter.write_str("yield requires a direct value-producing arm")
            }
            Self::MissingBranchValue => {
                formatter.write_str("neither conditional arm yields a value")
            }
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
    kind: FunctionKind,
}

enum FunctionKind {
    Defined(Option<Body>),
    Imported { module: String, name: String },
}

struct Body {
    values: Vec<Value>,
    region: Region,
}

enum Terminal {
    Yield(usize),
    Return(usize),
    TailCall(Invocation),
}

impl Terminal {
    fn inputs(&self) -> &[usize] {
        match self {
            Self::Yield(value) | Self::Return(value) => std::slice::from_ref(value),
            Self::TailCall(invocation) => &invocation.arguments,
        }
    }
}

enum Operation {
    Load(usize),
    Store {
        location: Location,
        value: usize,
    },
    If {
        condition: usize,
        branch: Region,
        else_branch: Option<Region>,
        output: Option<usize>,
    },
    Call {
        invocation: Invocation,
        output: usize,
    },
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
    Binary(BinaryOp, usize, usize),
    Shift {
        operator: ShiftOp,
        value: usize,
        count: usize,
    },
    Select {
        condition: usize,
        when_true: usize,
        when_false: usize,
    },
    SignExtend(usize),
    Compare(CompareOp, usize, usize),
    ZeroTest {
        input: usize,
        nonzero: bool,
    },
    Convert(usize),
    Normalize(usize),
    Load {
        location: Location,
        site: Site,
    },
    CallResult {
        site: Site,
    },
    JoinResult {
        site: Site,
    },
}

/// Builds a function body or a conditional branch. A yield, return or tail call
/// consumes the active builder; completing the outer builder saves the function
/// body.
///
/// Dropping the outer builder without completing it leaves the function undefined.
/// Dropping a child of `if_` or `if_else` completes a branch that falls through.
/// A value-producing arm must instead yield a value, return, or tail-call.
/// A builder cannot be used after its program is consumed:
/// ```compile_fail
/// use wasm86_compiler::{Program, Signature, Type, I32};
/// let mut program = Program::new();
/// let function = program.declare(Signature { parameters: vec![], result: Type::I32 });
/// let body = program.define(function).unwrap();
/// let module = program.compile();
/// body.return_(0).unwrap();
/// ```
pub struct FunctionBuilder<'p> {
    program: &'p mut Program,
    function: Func,
    arena: ExpressionArena,
    region: Region,
    destination: Destination<'p>,
    // A consuming terminal disables implicit fallthrough before validation can fail.
    fallthrough: bool,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares and builds a function, returning its handle after completion.
    /// The callback must complete the outer body with a return or tail call.
    /// An error or an incomplete body removes the new declaration, leaving this
    /// program usable. Use [`Self::declare`] and [`Self::define`] for forward
    /// references or recursive functions.
    pub fn function(
        &mut self,
        signature: Signature,
        build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Func, BuildError> {
        let function = self.declare(signature);
        let result = self.define(function).and_then(build).and_then(|()| {
            if matches!(
                self.functions[function.0].kind,
                FunctionKind::Defined(Some(_))
            ) {
                Ok(function)
            } else {
                Err(BuildError::MissingBody)
            }
        });
        if result.is_err() {
            // The body holds the program borrow, so the callback cannot append
            // declarations after this one.
            self.functions.pop();
        }
        result
    }

    /// Declares a function that must be defined before compilation.
    /// Definitions are emitted in declaration order, including unexported functions.
    pub fn declare(&mut self, signature: Signature) -> Func {
        let function = Func(self.functions.len());
        self.functions.push(Declaration {
            signature,
            kind: FunctionKind::Defined(None),
        });
        function
    }

    /// Starts a body for a function that has no completed definition.
    pub fn define(&mut self, function: Func) -> Result<FunctionBuilder<'_>, BuildError> {
        let declaration = self
            .functions
            .get(function.0)
            .ok_or(BuildError::UnknownFunction)?;
        match declaration.kind {
            FunctionKind::Imported { .. } => return Err(BuildError::ImportedFunction),
            FunctionKind::Defined(Some(_)) => return Err(BuildError::AlreadyDefined),
            FunctionKind::Defined(None) => {}
        }
        Ok(FunctionBuilder {
            program: self,
            function,
            arena: ExpressionArena::new(),
            region: Region::new(0),
            destination: Destination::Function,
            fallthrough: false,
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
    /// Every defined function must have a completed body.
    pub fn compile(self) -> Result<Vec<u8>, BuildError> {
        if self
            .functions
            .iter()
            .any(|function| matches!(function.kind, FunctionKind::Defined(None)))
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

    /// Obtains a typed value to retain or use in an expression. Existing values
    /// must belong to this body and be visible in the active branch; they keep
    /// their original expression and sharing. Literals follow [`Argument`]'s rules.
    /// Stores, conditions and returns also accept literals directly.
    ///
    /// ```compile_fail
    /// use wasm86_compiler::{FunctionBuilder, I32};
    /// fn retain_wide_literal(body: &FunctionBuilder<'_>) {
    ///     let value = body.value::<I32>(1_u64);
    /// }
    /// ```
    pub fn value<T: IntType>(&self, operand: impl IntoOp<T>) -> Result<Val<T>, BuildError> {
        let value = self.operand(operand)?;
        Ok(Val::new(self.arena.clone(), Ok(value)))
    }

    /// Returns this value from the generated function, consuming the active builder.
    /// A literal uses the signature's result type; a typed value must match it.
    /// In a branch, only that branch is completed. Completing the outer builder
    /// saves the function body; an error there leaves the function undefined.
    pub fn return_(mut self, result: impl Into<Argument>) -> Result<(), BuildError> {
        self.fallthrough = false;
        let result = self.argument(result, self.signature().result)?;
        let result = self.arena.normalize(result)?;
        self.complete(Terminal::Return(result))
    }

    fn operand<T: IntType>(&self, value: impl IntoOp<T>) -> Result<usize, BuildError> {
        self.argument(value, T::TYPE)
    }

    fn argument(&self, value: impl Into<Argument>, expected: Type) -> Result<usize, BuildError> {
        let value = value.into().resolve(&self.arena, expected)?;
        self.arena.require_visible(value, self.region.id)?;
        Ok(value)
    }

    fn site(&self) -> Site {
        Site {
            region: self.region.id,
            index: self.region.operations.len(),
        }
    }

    fn complete(mut self, terminal: Terminal) -> Result<(), BuildError> {
        self.fallthrough = false;
        self.region.terminal = Some(terminal);
        let region = std::mem::replace(&mut self.region, Region::new(0));
        match &mut self.destination {
            Destination::Function => {
                let values = self.arena.take().ok_or(BuildError::BodyClosed)?;
                self.program.functions[self.function.0].kind =
                    FunctionKind::Defined(Some(Body { values, region }));
            }
            Destination::Branch {
                region: destination,
                ..
            } => **destination = Some(region),
        }
        Ok(())
    }
}

impl Drop for FunctionBuilder<'_> {
    fn drop(&mut self) {
        match &mut self.destination {
            Destination::Function => {
                self.arena.take();
            }
            Destination::Branch {
                region: destination,
                result: None,
            } if self.fallthrough => {
                **destination = Some(std::mem::replace(&mut self.region, Region::new(0)));
            }
            Destination::Branch { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildError, MemoryImport, Program, Signature, Type, I1, I32};
    use wasmparser::{Operator, Parser, Payload, Validator};

    #[test]
    fn completed_functions_can_be_called_and_exported_with_logical_signatures() {
        let mut program = Program::new();
        let is_zero = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    result: Type::I1,
                },
                |body| {
                    let value = body.parameter::<I32>(0)?;
                    body.return_(value.eq(0))
                },
            )
            .unwrap();
        let run = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    result: Type::I1,
                },
                |mut body| {
                    let input = body.parameter::<I32>(0)?;
                    let result = body.call::<I1>(is_zero, &[input.into()])?;
                    body.return_(result)
                },
            )
            .unwrap();
        program.export("is_zero", is_zero).unwrap();
        program.export("run", run).unwrap();
        let bytes = program.compile().unwrap();
        Validator::new().validate_all(&bytes).unwrap();
        let calls = Parser::new(0)
            .parse_all(&bytes)
            .filter_map(|payload| match payload.unwrap() {
                Payload::CodeSectionEntry(body) => Some(
                    body.get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .filter(|operator| matches!(operator, Ok(Operator::Call { .. })))
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>();
        assert_eq!(calls, 1);
    }

    #[test]
    fn a_callback_error_discards_even_a_completed_body_and_its_import_use() {
        let mut program = Program::new();
        let memory = program.import_memory(MemoryImport {
            module: "host".into(),
            name: "memory".into(),
            minimum: 1,
            maximum: None,
        });
        let error = program
            .function(
                Signature {
                    parameters: vec![],
                    result: Type::I32,
                },
                |mut body| {
                    let value = body.load::<I32>(memory, 0)?;
                    body.return_(value)?;
                    Err(BuildError::BodyClosed)
                },
            )
            .unwrap_err();
        assert_eq!(error, BuildError::BodyClosed);
        let function = program
            .function(
                Signature {
                    parameters: vec![],
                    result: Type::I32,
                },
                |body| body.return_(7),
            )
            .unwrap();
        assert_eq!(function.0, 0);
        program.export("run", function).unwrap();
        let bytes = program.compile().unwrap();
        Validator::new().validate_all(&bytes).unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
    }

    #[test]
    fn a_successful_callback_must_complete_its_body() {
        let mut program = Program::new();
        let error = program
            .function(
                Signature {
                    parameters: vec![],
                    result: Type::I1,
                },
                |_body| Ok(()),
            )
            .unwrap_err();
        assert_eq!(error, BuildError::MissingBody);
        let function = program
            .function(
                Signature {
                    parameters: vec![],
                    result: Type::I1,
                },
                |body| body.return_(true),
            )
            .unwrap();
        assert_eq!(function.0, 0);
        program.export("run", function).unwrap();
        Validator::new()
            .validate_all(&program.compile().unwrap())
            .unwrap();
    }
}
