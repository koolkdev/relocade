use crate::{
    place, Body, BuildError, FunctionBuilder, IntType, Operation, Program, Type, Val, ValueKind,
    I16, I32, I64, I8,
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
    pub(super) base: usize,
    pub(super) offset: u32,
    pub(super) bytes: u8,
}

impl Location {
    fn new(memory: Mem, base: usize, offset: u32, ty: Type) -> Self {
        let bytes = match ty {
            Type::I8 => 1,
            Type::I16 => 2,
            Type::I32 => 4,
            Type::I64 => 8,
            Type::I1 => unreachable!("memory operations require a whole-byte integer type"),
        };
        Self {
            memory,
            base,
            offset,
            bytes,
        }
    }

    pub(super) fn may_overlap(self, other: Self, body: &Body) -> bool {
        if self.memory != other.memory {
            return false;
        }
        let left = place::representation(body, self.base);
        let right = place::representation(body, other.base);
        let (left_start, right_start) = match (body.values[left].kind, body.values[right].kind) {
            (ValueKind::Constant(a), ValueKind::Constant(b)) => {
                (a + u64::from(self.offset), b + u64::from(other.offset))
            }
            _ if left == right => (u64::from(self.offset), u64::from(other.offset)),
            _ => return true,
        };
        // Displacements add without wrapping, so equal bases preserve disjoint
        // spans. Different unknown bases may still name the same bytes.
        left_start < right_start + u64::from(other.bytes)
            && right_start < left_start + u64::from(self.bytes)
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
        let address = self.constant::<I32>(0);
        self.load_at(memory, &address, offset)
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
    ///     module: "guest".into(), name: "memory".into(), minimum: 1, maximum: None,
    /// });
    /// let function = program.declare(Signature {
    ///     parameters: vec![Type::I32], result: Type::I8,
    /// });
    /// let mut body = program.define(function)?;
    /// let address = body.parameter::<I32>(0)?;
    /// let byte = body.load_at::<I8>(memory, &address, 1)?;
    /// body.return_(&byte)?;
    /// program.export("next_byte", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn load_at<T: MemoryInt>(
        &mut self,
        memory: Mem,
        address: &Val<I32>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        let base = address.admit(&self.arena)?;
        self.require_memory(memory)?;
        let location = Location::new(memory, base, offset, T::TYPE);
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
        let address = self.constant::<I32>(0);
        self.store_at(memory, &address, offset, value)
    }

    /// Writes at an unsigned 32-bit address plus a constant byte displacement,
    /// with the same nonwrapping displacement rule as [`Self::load_at`].
    /// Stores keep their order. When both operands still need evaluation,
    /// the address is evaluated first.
    pub fn store_at<T: MemoryInt>(
        &mut self,
        memory: Mem,
        address: &Val<I32>,
        offset: u32,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        let base = address.admit(&self.arena)?;
        let value = value.admit(&self.arena)?;
        self.require_memory(memory)?;
        self.operations.push(Operation::Store {
            location: Location::new(memory, base, offset, T::TYPE),
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
    fn a_foreign_store_operand_leaves_the_body_usable_and_does_not_retain_an_import() {
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
        assert_eq!(
            body.store_at(memory, &foreign, 0, &result),
            Err(BuildError::ForeignBody)
        );
        body.return_(&result).unwrap();
        let bytes = program.compile().unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|payload| !matches!(payload.unwrap(), Payload::ImportSection(_))));
    }
}
