//! Naturally aligned atomic effects with explicit sequentially consistent ordering.

use std::marker::PhantomData;

use super::{Mem, MemoryAccess, MemoryInt};
use crate::{body::Operation, BlockBuilder, BuildError, Val, I32};

/// An atomic access at a fixed logical width and address. Atomic operations are
/// sequentially consistent, execute once in authored order, and require natural
/// alignment at runtime. Misaligned or out-of-bounds accesses trap.
/// The same operations work with shared or private memories. An unused result
/// does not discard the access or its synchronization effects.
pub struct AtomicAccess<'body, 'program, T: MemoryInt> {
    body: &'body mut BlockBuilder<'program>,
    access: MemoryAccess,
    address: usize,
    width: PhantomData<T>,
}

#[derive(Clone, Copy)]
pub(crate) enum AtomicKind {
    Load,
    Store,
    CompareExchange,
    Update(AtomicUpdate),
}

/// Read-modify-write operators that take one value and return the prior memory value.
#[derive(Clone, Copy)]
pub(crate) enum AtomicUpdate {
    Add,
    Subtract,
    And,
    Or,
    Xor,
    Exchange,
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
            access: MemoryAccess::new::<T>(memory, offset),
            address: base,
            width: PhantomData,
        })
    }

    /// Orders memory effects in all imported memories, including accesses made
    /// by generated helpers. The fence remains present when no value is used.
    pub fn atomic_fence(&mut self) {
        self.execute(Operation::fence(), &[])
            .expect("an active builder owns an open body");
    }
}

impl<T: MemoryInt> AtomicAccess<'_, '_, T> {
    /// Reads the operand at its logical width.
    pub fn load(self) -> Result<Val<T>, BuildError> {
        let operation = Operation::atomic_load(self.access, self.address);
        self.value(operation)
    }

    /// Writes the low bits at the operand's logical width.
    pub fn store(self, value: impl Into<Val<T>>) -> Result<(), BuildError> {
        let value = self.body.operand(value)?;
        self.body.execute(
            Operation::atomic_store(self.access, self.address, value),
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
        let operation =
            Operation::atomic_compare_exchange(self.access, self.address, expected, replacement);
        self.value(operation)
    }

    /// Adds modulo the operand width and returns the previous value.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        self.update(AtomicUpdate::Add, value)
    }

    /// Subtracts modulo the operand width and returns the previous value.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        self.update(AtomicUpdate::Subtract, value)
    }

    /// Applies bitwise AND and returns the previous value.
    pub fn and(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        self.update(AtomicUpdate::And, value)
    }

    /// Applies bitwise OR and returns the previous value.
    pub fn or(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        self.update(AtomicUpdate::Or, value)
    }

    /// Applies bitwise XOR and returns the previous value.
    pub fn xor(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        self.update(AtomicUpdate::Xor, value)
    }

    /// Replaces the value and returns the previous value.
    pub fn exchange(self, value: impl Into<Val<T>>) -> Result<Val<T>, BuildError> {
        self.update(AtomicUpdate::Exchange, value)
    }

    fn update(
        self,
        operator: AtomicUpdate,
        value: impl Into<Val<T>>,
    ) -> Result<Val<T>, BuildError> {
        let value = self.body.operand(value)?;
        let operation = Operation::atomic_update(self.access, operator, self.address, value);
        self.value(operation)
    }

    fn value(self, operation: Operation) -> Result<Val<T>, BuildError> {
        let output = self.body.execute(operation, &[T::TYPE])?[0];
        Ok(Val::new(self.body.arena.clone(), Ok(output)))
    }
}
