use crate::{
    body::Operation, AtLeast, BlockBuilder, BuildError, Program, Val, ValueType, F64, I1, I16, I32,
    I64, I8, V128,
};

mod atomic;
mod bulk;
pub use atomic::AtomicAccess;
pub(super) use atomic::{AtomicKind, AtomicUpdate};

/// An imported memory. Use only with the program that declared it.
/// Distinct declarations must be bound to distinct WebAssembly memory objects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Mem(pub(super) usize);

/// The external name and limits of a WebAssembly memory import.
/// Limits are in 64-KiB pages and must be valid for a 32-bit memory: at most
/// 65536 pages, with a maximum no smaller than the minimum.
pub struct MemoryImport {
    pub module: String,
    pub name: String,
    pub minimum: u32,
    pub maximum: Option<u32>,
    /// Whether the host supplies a shared memory. Requires an explicit maximum.
    /// Sharing does not change ordinary load/store ordering or atomicity.
    pub shared: bool,
}

/// An integer type supported by memory accesses at its logical width.
/// Loads and stores access exactly 1, 2, 4 or 8 bytes for I8, I16, I32 or I64.
/// These types also support narrowing to an individual logical bit.
/// Storing an I1 value in a larger slot is a separate, unsupported operation.
/// ```compile_fail
/// use wasm86_compiler::{BlockBuilder, Mem, I1};
/// fn load_bit(body: &mut BlockBuilder<'_>, memory: Mem) {
///     let bit = body.load::<I1>(memory, 0);
/// }
/// ```
pub trait MemoryInt: MemoryType + AtLeast<I1> + AtLeast<I8> {}

/// A value type stored in a whole number of bytes. Floating loads and stores
/// preserve the exact encoding, including signed zeros and NaN payloads.
pub trait MemoryType: ValueType {
    /// The number of bytes read or written by an access of this type.
    const BYTES: u32;
}

impl MemoryType for I8 {
    const BYTES: u32 = 1;
}
impl MemoryType for I16 {
    const BYTES: u32 = 2;
}
impl MemoryType for I32 {
    const BYTES: u32 = 4;
}
impl MemoryType for I64 {
    const BYTES: u32 = 8;
}

impl MemoryType for F64 {
    const BYTES: u32 = 8;
}

impl MemoryType for V128 {
    const BYTES: u32 = 16;
}

impl MemoryInt for I8 {}
impl MemoryInt for I16 {}
impl MemoryInt for I32 {}
impl MemoryInt for I64 {}

/// Memory attributes independent of the address supplied by an operation.
#[derive(Clone, Copy)]
pub(super) struct MemoryAccess {
    pub(super) memory: Mem,
    pub(super) offset: u32,
    pub(super) bytes: u8,
}

impl MemoryAccess {
    fn new<T: MemoryType>(memory: Mem, offset: u32) -> Self {
        Self {
            memory,
            offset,
            bytes: T::BYTES as u8,
        }
    }

    pub(super) fn at(self, base: usize) -> Location {
        Location {
            memory: self.memory,
            base,
            offset: self.offset,
            bytes: self.bytes,
        }
    }
}

/// A memory access with its current address input, used for overlap analysis.
#[derive(Clone, Copy)]
pub(super) struct Location {
    pub(super) memory: Mem,
    pub(super) base: usize,
    pub(super) offset: u32,
    pub(super) bytes: u8,
}

impl Program {
    /// Declares an imported memory. It is emitted only if a completed body
    /// contains a memory access naming it, including an unused ordinary load.
    /// Panics if the limits are invalid or a shared memory has no maximum.
    pub fn import_memory(&mut self, import: MemoryImport) -> Mem {
        assert!(
            import.minimum <= 65536,
            "memory minimum exceeds 32-bit limits"
        );
        assert!(
            import
                .maximum
                .is_none_or(|maximum| maximum >= import.minimum && maximum <= 65536),
            "invalid memory maximum"
        );
        assert!(
            !import.shared || import.maximum.is_some(),
            "shared memory requires a maximum"
        );
        let memory = Mem(self.memories.len());
        self.memories.push(import);
        memory
    }
}

impl BlockBuilder<'_> {
    /// Reads a value at a fixed byte offset in little-endian memory.
    /// Each call creates a separate read. Reusing its value preserves that read's
    /// snapshot across overlapping stores and explicit atomic effects. A used
    /// read may run later, past stores to other bytes; an unused read and its
    /// possible trap are omitted.
    pub fn load<T: MemoryType>(&mut self, memory: Mem, offset: u32) -> Result<Val<T>, BuildError> {
        self.load_at(memory, 0, offset)
    }

    /// Reads at an unsigned 32-bit address plus a constant byte displacement.
    /// The displacement addition does not wrap; an executed out-of-bounds access
    /// traps. Use `address.add(amount)` to request wrapping base arithmetic.
    /// The snapshot and reordering rules of [`Self::load`] also apply here.
    ///
    /// ```
    /// use wasm86_compiler::{MemoryImport, Program, Signature, Type, I8, I32};
    /// let mut program = Program::new();
    /// let memory = program.import_memory(MemoryImport {
    ///     module: "guest".into(), name: "memory".into(), minimum: 1, maximum: None, shared: false,
    /// });
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I8],
    /// }, |mut body| {
    ///     let address = body.parameter::<I32>(0)?;
    ///     let byte = body.load_at::<I8>(memory, &address, 1)?;
    ///     body.return_(&byte)
    /// })?;
    /// program.export("next_byte", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn load_at<T: MemoryType>(
        &mut self,
        memory: Mem,
        address: impl Into<Val<I32>>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        let base = self.operand(address)?;
        self.require_memory(memory)?;
        let access = MemoryAccess::new::<T>(memory, offset);
        let value = self.execute(Operation::load(access, base), &[T::TYPE])?[0];
        Ok(Val::new(self.arena.clone(), Ok(value)))
    }

    /// Writes the value's encoding in little-endian byte order. Stores execute
    /// in the order they are constructed and access exactly the type's byte size.
    pub fn store<T: MemoryType>(
        &mut self,
        memory: Mem,
        offset: u32,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        self.store_at(memory, 0, offset, value)
    }

    /// Writes at an unsigned 32-bit address plus a constant byte displacement,
    /// with the same nonwrapping displacement rule as [`Self::load_at`].
    /// Stores keep their order. When both operands still need evaluation,
    /// the address is evaluated first.
    pub fn store_at<T: MemoryType>(
        &mut self,
        memory: Mem,
        address: impl Into<Val<I32>>,
        offset: u32,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        let base = self.operand(address)?;
        let value = self.operand(value)?;
        self.require_memory(memory)?;
        self.execute(
            Operation::store(MemoryAccess::new::<T>(memory, offset), base, value),
            &[],
        )?;
        Ok(())
    }

    fn require_memory(&self, memory: Mem) -> Result<(), BuildError> {
        self.program
            .memories
            .get(memory.0)
            .ok_or(BuildError::UnknownMemory)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryImport;
    use crate::{BuildError, Program, Signature, Type, I32};
    use wasmparser::{Parser, Payload};

    #[test]
    fn foreign_memory_operands_leave_the_body_usable_and_do_not_retain_an_import() {
        let mut program = Program::new();
        let memory = program.import_memory(MemoryImport {
            module: "state".into(),
            name: "memory".into(),
            minimum: 1,
            maximum: None,
            shared: false,
        });
        let function = program.declare(Signature {
            parameters: vec![],
            results: vec![Type::I32],
        });
        let mut foreign = None;
        assert_eq!(
            program.define(function, |discarded| {
                foreign = Some(discarded.value::<I32>(9).unwrap());
                Ok(())
            }),
            Err(BuildError::MissingBody)
        );
        let foreign = foreign.unwrap();

        program
            .define(function, |mut body| {
                assert_eq!(
                    body.store(memory, 0, &foreign),
                    Err(BuildError::ForeignBody)
                );
                assert_eq!(
                    body.store_at::<I32>(memory, &foreign, 0, 7),
                    Err(BuildError::ForeignBody)
                );
                assert_eq!(
                    body.atomic::<I32>(memory, &foreign, 0).err(),
                    Some(BuildError::ForeignBody)
                );
                assert_eq!(
                    body.atomic::<I32>(memory, 0, 0).unwrap().store(&foreign),
                    Err(BuildError::ForeignBody)
                );
                assert_eq!(
                    body.atomic::<I32>(memory, 0, 0)
                        .unwrap()
                        .add(&foreign)
                        .err(),
                    Some(BuildError::ForeignBody)
                );
                let local = body.value::<I32>(7).unwrap();
                for [destination, value, bytes] in [
                    [&foreign, &local, &local],
                    [&local, &foreign, &local],
                    [&local, &local, &foreign],
                ] {
                    assert_eq!(
                        body.memory_fill(memory, destination, value, bytes),
                        Err(BuildError::ForeignBody)
                    );
                    assert_eq!(
                        body.memory_copy(memory, destination, memory, value, bytes),
                        Err(BuildError::ForeignBody)
                    );
                }

                for (expected, replacement) in [(&foreign, &local), (&local, &foreign)] {
                    assert_eq!(
                        body.atomic::<I32>(memory, 0, 0)
                            .unwrap()
                            .compare_exchange(expected, replacement)
                            .err(),
                        Some(BuildError::ForeignBody)
                    );
                }
                body.return_(7)
            })
            .unwrap();
        let bytes = program.compile().unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
    }
}
