//! Naturally aligned atomic effects with explicit sequentially consistent ordering.

use std::marker::PhantomData;

use super::{Location, Mem, MemoryInt};
use crate::{BuildError, FunctionBuilder, Operation, Val, I32};

/// An atomic access at a fixed logical width and address. Atomic operations are
/// sequentially consistent, execute once in authored order, and require natural
/// alignment at runtime. Misaligned or out-of-bounds accesses trap.
/// The same operations work with shared or private memories. An unused result
/// does not discard the access or its synchronization effects.
pub struct AtomicAccess<'body, 'program, T: MemoryInt> {
    body: &'body mut FunctionBuilder<'program>,
    location: Location,
    width: PhantomData<T>,
}

#[derive(Clone, Copy)]
pub(crate) enum AtomicKind {
    Load,
    Store { value: usize },
    CompareExchange { expected: usize, replacement: usize },
    Add(usize),
    Subtract(usize),
    And(usize),
    Or(usize),
    Xor(usize),
    Exchange(usize),
}

pub(crate) struct AtomicOperation {
    pub(crate) location: Location,
    pub(crate) operation: AtomicKind,
}

impl AtomicOperation {
    pub(crate) fn inputs(&self) -> impl Iterator<Item = usize> {
        let (first, second) = match self.operation {
            AtomicKind::Load => (None, None),
            AtomicKind::Store { value }
            | AtomicKind::Add(value)
            | AtomicKind::Subtract(value)
            | AtomicKind::And(value)
            | AtomicKind::Or(value)
            | AtomicKind::Xor(value)
            | AtomicKind::Exchange(value) => (Some(value), None),
            AtomicKind::CompareExchange {
                expected,
                replacement,
            } => (Some(expected), Some(replacement)),
        };
        [Some(self.location.base), first, second]
            .into_iter()
            .flatten()
    }
}

impl<'program> FunctionBuilder<'program> {
    /// Selects an atomic memory operand. Address displacement does not wrap,
    /// matching ordinary memory accesses. Operations accept native literals.
    ///
    /// ```
    /// use wasm86_compiler::{MemoryImport, Program, Signature, Type, I32};
    /// let mut program = Program::new();
    /// let memory = program.import_memory(MemoryImport {
    ///     module: "host".into(), name: "counter".into(),
    ///     minimum: 1, maximum: Some(1), shared: true,
    /// });
    /// let increment = program.function(Signature {
    ///     parameters: vec![], results: vec![Type::I32],
    /// }, |mut body| {
    ///     let previous = body.atomic::<I32>(memory, 0, 0)?.add(1)?;
    ///     body.return_(previous)
    /// })?;
    /// program.export("increment", increment)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn atomic<T: MemoryInt>(
        &mut self,
        memory: Mem,
        address: impl Into<Val<I32>>,
        offset: u32,
    ) -> Result<AtomicAccess<'_, 'program, T>, BuildError> {
        let base = self.operand(address)?;
        self.require_memory(memory)?;
        Ok(AtomicAccess {
            body: self,
            location: Location::new::<T>(memory, base, offset),
            width: PhantomData,
        })
    }

    /// Orders memory effects in all imported memories, including accesses made
    /// by generated helpers. The fence remains present when no value is used.
    pub fn atomic_fence(&mut self) {
        self.region.operations.push(Operation::Fence);
    }
}

impl<T: MemoryInt> AtomicAccess<'_, '_, T> {
    /// Reads the operand at its logical width.
    pub fn load(self) -> Result<Val<T>, BuildError> {
        self.value(AtomicKind::Load)
    }

    /// Writes the low bits at the operand's logical width.
    pub fn store(self, value: impl Into<Val<T>>) -> Result<(), BuildError> {
        let value = self.body.operand(value)?;
        self.body.region.operations.push(Operation::Atomic {
            access: AtomicOperation {
                location: self.location,
                operation: AtomicKind::Store { value },
            },
            output: None,
        });
        Ok(())
    }

    /// Returns the previous value, whether or not the comparison succeeds.
    pub fn compare_exchange(
        self,
        expected: impl Into<Val<T>>,
        replacement: impl Into<Val<T>>,
    ) -> Result<Val<T>, BuildError> {
        let expected = self.body.operand(expected)?;
        let replacement = self.body.operand(replacement)?;
        self.value(AtomicKind::CompareExchange {
            expected,
            replacement,
        })
    }

    /// Adds modulo the operand width and returns the previous value.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        let value = self.body.operand(value)?;
        self.value(AtomicKind::Add(value))
    }

    /// Subtracts modulo the operand width and returns the previous value.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        let value = self.body.operand(value)?;
        self.value(AtomicKind::Subtract(value))
    }

    /// Applies bitwise AND and returns the previous value.
    pub fn and(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        let value = self.body.operand(value)?;
        self.value(AtomicKind::And(value))
    }

    /// Applies bitwise OR and returns the previous value.
    pub fn or(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        let value = self.body.operand(value)?;
        self.value(AtomicKind::Or(value))
    }

    /// Applies bitwise XOR and returns the previous value.
    pub fn xor(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        let value = self.body.operand(value)?;
        self.value(AtomicKind::Xor(value))
    }

    /// Replaces the value and returns the previous value.
    pub fn exchange(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        let value = self.body.operand(value)?;
        self.value(AtomicKind::Exchange(value))
    }

    fn value(self, operation: AtomicKind) -> Result<Val<T>, BuildError> {
        let output = self
            .body
            .arena
            .operation_result(T::TYPE, self.body.site(), 0)?;
        self.body.region.operations.push(Operation::Atomic {
            access: AtomicOperation {
                location: self.location,
                operation,
            },
            output: Some(output),
        });
        Ok(Val::new(self.body.arena.clone(), Ok(output)))
    }
}
