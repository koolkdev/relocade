//! Naturally aligned atomic effects with explicit sequentially consistent ordering.

use std::marker::PhantomData;

use super::{Location, Mem, MemoryInt};
use crate::{body::Operation, BlockBuilder, BuildError, Val, I32};

/// An atomic access at a fixed logical width and address. Atomic operations are
/// sequentially consistent, execute once in authored order, and require natural
/// alignment at runtime. Misaligned or out-of-bounds accesses trap.
/// The same operations work with shared or private memories. An unused result
/// does not discard the access or its synchronization effects.
pub struct AtomicAccess<'body, 'program, T: MemoryInt> {
    body: &'body mut BlockBuilder<'program>,
    location: Location,
    width: PhantomData<T>,
}

#[derive(Clone, Copy)]
pub(crate) enum AtomicKind<V = usize> {
    Load,
    Store { value: V },
    CompareExchange { expected: V, replacement: V },
    Add(V),
    Subtract(V),
    And(V),
    Or(V),
    Xor(V),
    Exchange(V),
}

#[derive(Clone, Copy)]
pub(crate) struct AtomicOperation<V = usize> {
    pub(crate) location: Location<V>,
    pub(crate) operation: AtomicKind<V>,
}

impl<V> AtomicKind<V> {
    fn map<U>(self, mut map: impl FnMut(V) -> U) -> AtomicKind<U> {
        match self {
            Self::Load => AtomicKind::Load,
            Self::Store { value } => AtomicKind::Store { value: map(value) },
            Self::CompareExchange {
                expected,
                replacement,
            } => AtomicKind::CompareExchange {
                expected: map(expected),
                replacement: map(replacement),
            },
            Self::Add(value) => AtomicKind::Add(map(value)),
            Self::Subtract(value) => AtomicKind::Subtract(map(value)),
            Self::And(value) => AtomicKind::And(map(value)),
            Self::Or(value) => AtomicKind::Or(map(value)),
            Self::Xor(value) => AtomicKind::Xor(map(value)),
            Self::Exchange(value) => AtomicKind::Exchange(map(value)),
        }
    }
}

impl<V> AtomicOperation<V> {
    pub(crate) fn map<U>(self, mut map: impl FnMut(V) -> U) -> AtomicOperation<U> {
        AtomicOperation {
            location: self.location.map(&mut map),
            operation: self.operation.map(map),
        }
    }
}

impl<V: Copy> AtomicOperation<V> {
    pub(crate) fn inputs(&self) -> impl Iterator<Item = V> {
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

impl<'program> BlockBuilder<'program> {
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
        self.execute(Operation::Fence, &[])
            .expect("an active builder owns an open body");
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
        self.body.execute(
            Operation::Atomic(AtomicOperation {
                location: self.location,
                operation: AtomicKind::Store { value },
            }),
            &[],
        )?;
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
        let output = self.body.execute(
            Operation::Atomic(AtomicOperation {
                location: self.location,
                operation,
            }),
            &[T::TYPE],
        )?[0];
        Ok(Val::new(self.body.arena.clone(), Ok(output)))
    }
}
