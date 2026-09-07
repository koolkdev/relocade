use crate::{
    BuildError, FunctionBuilder, IntType, Operation, Program, Type, Val, I16, I32, I64, I8,
};

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
}

/// An integer type supported by memory accesses at its logical width.
/// Loads and stores access exactly 1, 2, 4 or 8 bytes for I8, I16, I32 or I64.
/// Storing an I1 value in a larger slot is a separate, unsupported operation.
/// ```compile_fail
/// use wasm86_compiler::{FunctionBuilder, Mem, I1};
/// fn load_bit(body: &mut FunctionBuilder<'_>, memory: Mem) {
///     let bit = body.load::<I1>(memory, 0);
/// }
/// ```
pub trait MemoryInt: IntType {}

impl MemoryInt for I8 {}
impl MemoryInt for I16 {}
impl MemoryInt for I32 {}
impl MemoryInt for I64 {}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) struct Location {
    pub(super) memory: Mem,
    pub(super) offset: u32,
    pub(super) bytes: u8,
}

impl Location {
    fn new(memory: Mem, offset: u32, ty: Type) -> Self {
        let bytes = match ty {
            Type::I8 => 1,
            Type::I16 => 2,
            Type::I32 => 4,
            Type::I64 => 8,
            Type::I1 => unreachable!("memory operations require a whole-byte integer type"),
        };
        Self {
            memory,
            offset,
            bytes,
        }
    }

    pub(super) fn overlaps(self, other: Self) -> bool {
        self.memory == other.memory
            && u64::from(self.offset) < u64::from(other.offset) + u64::from(other.bytes)
            && u64::from(other.offset) < u64::from(self.offset) + u64::from(self.bytes)
    }
}

impl Program {
    /// Declares an imported memory. It is emitted only if a completed body
    /// contains a load or store naming it, including an unused load.
    pub fn import_memory(&mut self, import: MemoryImport) -> Mem {
        let memory = Mem(self.memories.len());
        self.memories.push(import);
        memory
    }
}

impl FunctionBuilder<'_> {
    /// Reads an integer at a fixed byte offset in little-endian memory.
    /// Each call creates a separate read. Reusing its value preserves that read's
    /// snapshot across overlapping stores. A used read may run later, past stores
    /// to other bytes; an unused read and its possible trap are omitted.
    pub fn load<T: MemoryInt>(&mut self, memory: Mem, offset: u32) -> Result<Val<T>, BuildError> {
        self.require_memory(memory)?;
        let location = Location::new(memory, offset, T::TYPE);
        let value = self.arena.load(T::TYPE, location, self.operations.len())?;
        self.operations.push(Operation::Load(value));
        Ok(Val::new(self.arena.clone(), Ok(value)))
    }

    /// Writes the value's low bits in little-endian byte order. Stores execute
    /// in the order they are constructed and access exactly the type's byte size.
    pub fn store<T: MemoryInt>(
        &mut self,
        memory: Mem,
        offset: u32,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        let value = value.admit(&self.arena)?;
        self.require_memory(memory)?;
        self.operations.push(Operation::Store {
            location: Location::new(memory, offset, T::TYPE),
            value,
        });
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
    fn a_foreign_store_leaves_the_body_usable_and_does_not_retain_an_import() {
        let mut program = Program::new();
        let memory = program.import_memory(MemoryImport {
            module: "state".into(),
            name: "memory".into(),
            minimum: 1,
            maximum: None,
        });
        let function = program.declare(Signature {
            parameters: vec![],
            result: Type::I32,
        });
        let discarded = program.define(function).unwrap();
        let foreign = discarded.constant::<I32>(9);
        drop(discarded);

        let mut body = program.define(function).unwrap();
        assert_eq!(
            body.store(memory, 0, &foreign),
            Err(BuildError::ForeignBody)
        );
        let result = body.constant::<I32>(7);
        body.return_(&result).unwrap();
        let bytes = program.compile().unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
    }
}
