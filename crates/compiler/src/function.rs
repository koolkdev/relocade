//! Function construction, pending blocks and definition publication.
use crate::{
    arena::ExpressionArena,
    control::{Block, JoinTarget, Site},
    Argument, Arguments, Body, BuildError, Func, FunctionKind, IntType, Operation, Program,
    Signature, Terminal, Type, Val, Value, ValueKind,
};

/// Builds a function body or child block. Its parent owns the block and attaches
/// it after the callback succeeds. Returns, yields, branches, tail calls and
/// traps consume the builder.
/// Ordinary child blocks may continue implicitly when the callback returns
/// `Ok(())`. A callback error discards its block.
///
/// Completing a block consumes its builder:
///
/// ```compile_fail
/// use wasm86_compiler::{Program, Signature, Type};
/// let mut program = Program::new();
/// program.function(Signature { parameters: vec![], results: vec![Type::I32] }, |body| {
///     body.return_(7)?;
///     body.return_(9)
/// });
/// ```
pub struct BlockBuilder<'program> {
    pub(super) program: &'program mut Program,
    pub(super) function: Func,
    pub(super) arena: ExpressionArena,
    pub(super) pending: &'program mut PendingBlock,
    pub(super) yield_target: Option<JoinTarget>,
}

// The parent owns this pending block and attaches it only after its callback succeeds.
// A failed terminal remains recorded even if the callback ignores its error.
pub(super) struct PendingBlock {
    pub(super) id: usize,
    pub(super) operations: Vec<Operation>,
    ending: Ending,
}

enum Ending {
    Fallthrough,
    Terminal(Terminal),
    Failed(BuildError),
}

impl PendingBlock {
    pub(super) fn new(id: usize) -> Self {
        Self {
            id,
            operations: Vec::new(),
            ending: Ending::Fallthrough,
        }
    }

    pub(super) fn finish(self) -> Result<Block, BuildError> {
        let terminal = match self.ending {
            Ending::Fallthrough => None,
            Ending::Terminal(terminal) => Some(terminal),
            Ending::Failed(error) => return Err(error),
        };
        Ok(Block {
            id: self.id,
            operations: self.operations,
            terminal,
        })
    }
}

impl Program {
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
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
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
        let result = self.define(function, build).map(|()| function);
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

    /// Builds a previously declared function. The callback must complete its
    /// supplied body with a return, tail call or trap. Errors leave the function
    /// undefined and close its values; it may be defined again.
    pub fn define(
        &mut self,
        function: Func,
        build: impl FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
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
        let mut definition = Definition {
            program: self,
            function,
            arena: ExpressionArena::new(),
        };
        let mut pending = PendingBlock::new(0);
        build(BlockBuilder {
            program: definition.program,
            function,
            arena: definition.arena.clone(),
            pending: &mut pending,
            yield_target: None,
        })?;
        definition.publish(pending.finish()?)
    }
}

// This owner closes the arena on every exit, including callback errors and unwinding.
// Drop performs cleanup only; the caller publishes after the callback succeeds.
struct Definition<'program> {
    program: &'program mut Program,
    function: Func,
    arena: ExpressionArena,
}

impl Definition<'_> {
    fn publish(&mut self, mut block: Block) -> Result<(), BuildError> {
        if block.terminal.is_none() {
            return Err(BuildError::MissingBody);
        }
        self.arena.simplify_paths(&mut block)?;
        let values = self.arena.take().ok_or(BuildError::BodyClosed)?;
        block.fold_constants(&values);
        self.program.functions[self.function.0].kind =
            FunctionKind::Defined(Some(Body { values, block }));
        Ok(())
    }
}

impl Drop for Definition<'_> {
    fn drop(&mut self) {
        self.arena.take();
        self.program.functions[self.function.0].building = false;
    }
}

impl BlockBuilder<'_> {
    /// Accesses this body's module to declare or build a helper when it is needed.
    /// Helper bodies have separate value arenas; this body remains open while
    /// the helper is built. The active function cannot be defined again.
    /// Declarations added inside a failing [`Program::function`] callback are
    /// rolled back with that callback, so discard their handles on failure.
    pub fn program(&mut self) -> &mut Program {
        self.program
    }

    pub(super) fn signature(&self) -> &Signature {
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
    /// use wasm86_compiler::{BlockBuilder, I32};
    /// fn retain_wide_literal(body: &BlockBuilder<'_>) {
    ///     let value = body.value::<I32>(1_u64);
    /// }
    /// ```
    pub fn value<T: IntType>(&self, operand: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        operand.into().bind(&self.arena, self.pending.id)
    }

    /// Returns values from the function, consuming the active builder.
    /// The signature determines literal types; typed values must match it.
    /// Scalars, tuples, arrays and vectors supply ordered results; `()` supplies none.
    /// In a branch, only that branch is completed. The parent attaches it after
    /// the callback succeeds. A callback error discards the completed block too.
    pub fn return_(self, arguments: impl Into<Arguments>) -> Result<(), BuildError> {
        self.terminate(|body| {
            let mut arguments = body.result_arguments(arguments, &body.signature().results)?;
            for argument in &mut arguments {
                *argument = body.arena.normalize(*argument)?;
            }
            Ok(Terminal::Return(arguments))
        })
    }

    /// Ends this execution path with a WebAssembly trap. This consumes the active
    /// builder and is valid for any function result type.
    pub fn trap(self) -> Result<(), BuildError> {
        self.terminate(|_| Ok(Terminal::Trap))
    }

    pub(super) fn operand<T: IntType>(
        &self,
        value: impl Into<Val<T>>,
    ) -> Result<usize, BuildError> {
        let value: Val<T> = value.into();
        value.checked_expression(&self.arena, self.pending.id)
    }

    pub(super) fn argument(
        &self,
        value: impl Into<Argument>,
        expected: Type,
    ) -> Result<usize, BuildError> {
        value.into().resolve(&self.arena, expected, self.pending.id)
    }

    pub(super) fn site(&self) -> Site {
        Site {
            block: self.pending.id,
            index: self.pending.operations.len(),
        }
    }

    pub(super) fn terminate(
        self,
        make_terminal: impl FnOnce(&Self) -> Result<Terminal, BuildError>,
    ) -> Result<(), BuildError> {
        match make_terminal(&self) {
            Ok(terminal) => {
                self.pending.ending = Ending::Terminal(terminal);
                Ok(())
            }
            Err(error) => {
                self.pending.ending = Ending::Failed(error.clone());
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
