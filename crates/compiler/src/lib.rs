//! Guest-independent construction of typed WebAssembly functions.
//!
//! Build functions, choose their exports, then compile the module:
//!
//! ```
//! use wasm86_compiler::{Program, Signature, Type, I32};
//!
//! let mut program = Program::new();
//! let increment = program.function(Signature {
//!     parameters: vec![Type::I32],
//!     results: vec![Type::I32],
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
mod results;
mod types;
mod value;

use std::fmt;

use arena::ExpressionArena;
pub use call::FunctionImport;
use call::Invocation;
use control::{Destination, Region, Site};
pub use control::{Label, LoopLabels};
use integer::{BinaryOp, BitCountOp, CompareOp, RotateOp, ShiftOp};
use memory::Location;
pub use memory::{Mem, MemoryImport, MemoryInt};
pub use results::{Arguments, Results};
pub use types::{AtLeast, IntType, Type, I1, I16, I32, I64, I8};
pub use value::{Argument, Signed, Unsigned, Val};

/// A function's ordered logical parameter and result types.
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
            Self::UnknownMemory => formatter.write_str("unknown memory declaration"),
            Self::UnknownFunction => formatter.write_str("unknown function declaration"),
            Self::AlreadyDefined => {
                formatter.write_str("function already has an open or finished body")
            }
            Self::MissingBody => formatter.write_str("function has no finished body"),
            Self::UnknownParameter => formatter.write_str("unknown function parameter"),
            Self::ForeignBody => formatter.write_str("value or label belongs to another body"),
            Self::OutOfScope => {
                formatter.write_str("value or label is not available in this scope")
            }
            Self::IncompleteBranch => formatter.write_str("branch termination did not complete"),
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
    building: bool,
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
    Trap,
    Branch {
        target: control::Target,
        arguments: Vec<usize>,
    },
    Return(Vec<usize>),
    TailCall(Invocation),
}

impl Terminal {
    fn inputs(&self) -> &[usize] {
        match self {
            Self::Trap => &[],
            Self::Return(arguments) | Self::Branch { arguments, .. } => arguments,
            Self::TailCall(invocation) => &invocation.arguments,
        }
    }
}

enum Operation {
    // Keep authored sites stable when control folding removes an operation.
    Nop,
    Load(usize),
    Store {
        location: Location,
        value: usize,
    },
    Block {
        region: Region,
        outputs: Vec<usize>,
    },
    Loop {
        initial: Vec<usize>,
        inputs: Vec<usize>,
        region: Region,
        outputs: Vec<usize>,
    },
    If {
        condition: usize,
        branch: Region,
        else_branch: Option<Region>,
        outputs: Vec<usize>,
    },
    BranchIf {
        condition: usize,
        taken: Region,
    },
    Switch {
        selector: usize,
        cases: Vec<control::SwitchCase>,
        default: Region,
        outputs: Vec<usize>,
    },
    Call {
        invocation: Invocation,
        outputs: Vec<usize>,
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
    LoopInput {
        region: usize,
        component: usize,
    },
    Binary(BinaryOp, usize, usize),
    Shift {
        operator: ShiftOp,
        value: usize,
        count: usize,
    },
    Rotate {
        operator: RotateOp,
        value: usize,
        count: usize,
    },
    Select {
        condition: usize,
        when_true: usize,
        when_false: usize,
    },
    SignExtend(usize),
    BitCount(BitCountOp, usize),
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
        component: usize,
    },
    JoinResult {
        site: Site,
        component: usize,
    },
}

/// Builds a function body, block, loop or branch. A yield, branch, return, tail call or trap
/// consumes the active builder; completing the outer builder saves the function
/// body.
///
/// Dropping the outer builder without completing it leaves the function undefined.
/// Dropping a child of `if_`, `if_else` or `switch` completes a branch that falls through.
/// A nonempty result body must instead yield, branch to a control label, return,
/// tail-call or trap. Blocks, loops and result arms with the unit shape may fall through.
/// A builder cannot be used after its program is consumed:
/// ```compile_fail
/// use wasm86_compiler::{Program, Signature, Type, I32};
/// let mut program = Program::new();
/// let function = program.declare(Signature { parameters: vec![], results: vec![Type::I32] });
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
    /// The callback must complete the outer body with a return, tail call or trap.
    /// An error or an incomplete body discards this function and any declarations,
    /// imports or exports added by its callback. Earlier forward declarations
    /// completed by the callback return to their undefined state. Handles created
    /// in a failed callback must be discarded too. Earlier declarations remain usable. Use
    /// [`Self::declare`] and [`Self::define`] for forward references or recursion.
    pub fn function(
        &mut self,
        signature: Signature,
        build: impl FnOnce(FunctionBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<Func, BuildError> {
        // Completed bodies are immutable. Only earlier forward declarations can
        // acquire a body during this callback, so rollback needs no IR copies.
        let undefined_functions: Vec<_> = self
            .functions
            .iter()
            .enumerate()
            .filter_map(|(index, declaration)| {
                matches!(declaration.kind, FunctionKind::Defined(None)).then_some(index)
            })
            .collect();
        let memory_count = self.memories.len();
        let export_count = self.exports.len();
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
            for index in undefined_functions {
                self.functions[index].kind = FunctionKind::Defined(None);
            }
            self.functions.truncate(function.0);
            self.memories.truncate(memory_count);
            self.exports.truncate(export_count);
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
            building: false,
        });
        function
    }

    /// Starts a body for a function that has neither an open nor completed definition.
    pub fn define(&mut self, function: Func) -> Result<FunctionBuilder<'_>, BuildError> {
        let declaration = self
            .functions
            .get_mut(function.0)
            .ok_or(BuildError::UnknownFunction)?;
        if declaration.building {
            return Err(BuildError::AlreadyDefined);
        }
        match declaration.kind {
            FunctionKind::Imported { .. } => return Err(BuildError::ImportedFunction),
            FunctionKind::Defined(Some(_)) => return Err(BuildError::AlreadyDefined),
            FunctionKind::Defined(None) => {}
        }
        declaration.building = true;
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
    /// Accesses this body's module to declare or build a helper when it is needed.
    /// Helper bodies have separate value arenas; this body remains open while
    /// the helper is built. The active function cannot be defined again.
    /// Declarations added inside a failing [`Program::function`] callback are
    /// rolled back with that callback, so discard their handles on failure.
    pub fn program(&mut self) -> &mut Program {
        self.program
    }

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

    /// Checks and binds a typed value to this body. Bound values must already
    /// belong to this body and be visible in the active branch; they keep their
    /// expression and sharing. Standalone literals and calculations are admitted
    /// as body expressions, sharing the body's constant and expression cache.
    /// Stores, conditions and returns also accept literals directly.
    ///
    /// ```compile_fail
    /// use wasm86_compiler::{FunctionBuilder, I32};
    /// fn retain_wide_literal(body: &FunctionBuilder<'_>) {
    ///     let value = body.value::<I32>(1_u64);
    /// }
    /// ```
    pub fn value<T: IntType>(&self, operand: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        operand.into().bind(&self.arena, self.region.id)
    }

    /// Returns values from the function, consuming the active builder.
    /// The signature determines literal types; typed values must match it.
    /// Scalars, tuples, arrays and vectors supply ordered results; `()` supplies none.
    /// In a branch, only that branch is completed. Completing the outer builder
    /// saves the function body; an error there leaves the function undefined.
    pub fn return_(mut self, arguments: impl Into<Arguments>) -> Result<(), BuildError> {
        self.fallthrough = false;
        let mut arguments = self.result_arguments(arguments, &self.signature().results)?;
        for argument in &mut arguments {
            *argument = self.arena.normalize(*argument)?;
        }
        self.complete(Terminal::Return(arguments))
    }

    /// Ends this execution path with a WebAssembly trap. This consumes the active
    /// builder and is valid for any function result type.
    pub fn trap(mut self) -> Result<(), BuildError> {
        self.fallthrough = false;
        self.complete(Terminal::Trap)
    }

    fn operand<T: IntType>(&self, value: impl Into<Val<T>>) -> Result<usize, BuildError> {
        let value: Val<T> = value.into();
        value.checked_expression(&self.arena, self.region.id)
    }

    fn argument(&self, value: impl Into<Argument>, expected: Type) -> Result<usize, BuildError> {
        value.into().resolve(&self.arena, expected, self.region.id)
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
        let mut region = std::mem::replace(&mut self.region, Region::new(0));
        match &mut self.destination {
            Destination::Function => {
                let values = self.arena.take().ok_or(BuildError::BodyClosed)?;
                region.fold_constants(&values);
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
                self.program.functions[self.function.0].building = false;
            }
            Destination::Branch {
                region: destination,
                target,
            } if self.fallthrough
                && target.as_ref().is_none_or(|target| target.types.is_empty()) =>
            {
                **destination = Some(std::mem::replace(&mut self.region, Region::new(0)));
            }
            Destination::Branch { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests;
