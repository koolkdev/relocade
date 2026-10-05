//! Function construction, pending blocks and definition publication.
use crate::{
    arena::FunctionArena,
    body::{BlockId, Exit, Layout, Operation},
    control::JoinTarget,
    Argument, Arguments, BuildError, Func, FunctionKind, IntType, Program, Signature, Type, Val,
    ValueType,
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
    pub(super) arena: FunctionArena,
    pub(super) pending: &'program mut PendingBlock,
    pub(super) yield_target: Option<JoinTarget>,
}

// The parent owns this pending block and attaches it only after its callback succeeds.
// A failed terminal remains recorded even if the callback ignores its error.
pub(super) struct PendingBlock {
    pub(super) id: usize,
    pub(super) entry: BlockId,
    pub(super) current: BlockId,
    pub(super) layout: Vec<Layout>,
    ending: Ending,
}

enum Ending {
    Fallthrough,
    Terminal,
    Failed(BuildError),
}

impl PendingBlock {
    pub(super) fn new(id: usize, block: BlockId) -> Self {
        Self {
            id,
            entry: block,
            current: block,
            layout: Vec::new(),
            ending: Ending::Fallthrough,
        }
    }

    pub(super) fn finish(mut self) -> Result<Self, BuildError> {
        if let Ending::Failed(error) = &self.ending {
            return Err(error.clone());
        }
        self.layout.push(Layout::Block(self.current));
        Ok(self)
    }

    pub(super) fn falls_through(&self) -> bool {
        matches!(self.ending, Ending::Fallthrough)
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
            arena: FunctionArena::new(),
        };
        for (component, ty) in definition.program.functions[function.0]
            .signature
            .parameters
            .iter()
            .copied()
            .enumerate()
        {
            definition.arena.parameter(ty, component)?;
        }
        let mut pending = PendingBlock::new(0, BlockId(0));
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
    arena: FunctionArena,
}

impl Definition<'_> {
    fn publish(&mut self, block: PendingBlock) -> Result<(), BuildError> {
        if block.falls_through() {
            return Err(BuildError::MissingBody);
        }
        let body = self.arena.finish(block.layout)?;
        self.program.functions[self.function.0].kind = FunctionKind::Defined(Some(body));
        Ok(())
    }
}

impl Drop for Definition<'_> {
    fn drop(&mut self) {
        self.arena.close();
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
    pub fn parameter<T: ValueType>(&self, index: u32) -> Result<Val<T>, BuildError> {
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
        let value = self.arena.parameter(actual, index as usize)?;
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
    pub fn value<T: ValueType>(&self, operand: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        operand.into().bind(&self.arena, self.pending.id)
    }

    /// Returns the zero-extended logical bits of an integer constant known during
    /// construction. Admits literals and calculations with the same ownership and
    /// branch visibility checks as [`Self::value`]. Runtime values and constants
    /// discovered later during placement return `None`.
    pub fn constant_bits<T: IntType>(
        &self,
        operand: impl Into<Val<T>>,
    ) -> Result<Option<u64>, BuildError> {
        let value = self.operand(operand)?;
        self.arena.constant_bits(value)
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
            Ok(Exit::Return(arguments))
        })
    }

    /// Ends this execution path with a WebAssembly trap. This consumes the active
    /// builder and is valid for any function result type.
    pub fn trap(self) -> Result<(), BuildError> {
        self.terminate(|_| Ok(Exit::Trap))
    }

    pub(super) fn operand<T: ValueType>(
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

    pub(super) fn execute(
        &mut self,
        operation: Operation,
        types: &[Type],
    ) -> Result<Vec<usize>, BuildError> {
        self.arena.execute(self.pending.current, operation, types)
    }

    pub(super) fn terminate(
        self,
        make_terminal: impl FnOnce(&Self) -> Result<Exit, BuildError>,
    ) -> Result<(), BuildError> {
        match make_terminal(&self) {
            Ok(terminal) => {
                self.arena.exit(self.pending.current, terminal)?;
                self.pending.ending = Ending::Terminal;
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
