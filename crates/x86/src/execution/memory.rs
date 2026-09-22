//! Checked memory operands support widths independently of register views.

use wasm86_compiler::{BuildError, MemoryInt, Val, I32};

use super::ExecutionBuilder;
use crate::{
    address::{self, MemoryAddress, RegisterValue},
    alu::OperandUpdate,
    memory::{Access, Intent, Memory},
    segment::SegmentSelection,
};

/// A resolved operand whose complete span passed segment and page checks.
/// Fields share that proof, so a later field cannot fault after an earlier write.
pub(super) struct MemoryOperand<'memory> {
    memory: &'memory Memory,
    access: Access,
    offset: Val<I32>,
    segment: SegmentSelection,
}

impl MemoryOperand<'_> {
    /// The address-sized effective offset, before adding the segment base.
    pub(super) fn offset(&self) -> &Val<I32> {
        &self.offset
    }

    pub(super) fn segment(&self) -> &SegmentSelection {
        &self.segment
    }

    pub(super) fn read<T: MemoryInt>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        self.memory
            .read::<T>(&mut execution.body, &self.access, offset)
    }

    pub(super) fn write<T: MemoryInt>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        offset: u32,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        let value = execution.body.value(value)?;
        self.memory
            .write(&mut execution.body, &self.access, offset, &value)
    }

    pub(super) fn atomic_update<T: MemoryInt>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        update: &OperandUpdate<T>,
    ) -> Result<Val<T>, BuildError> {
        self.memory
            .atomic_update(&mut execution.body, &self.access, update)
    }
}

impl<'memory> ExecutionBuilder<'_, 'memory> {
    pub(super) fn read_memory<T: MemoryInt>(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
    ) -> Result<Val<T>, BuildError> {
        self.memory_operand(address, T::BYTES, Intent::Read, &[])?
            .read(self, 0)
    }

    pub(super) fn prepare_memory_write<T: MemoryInt>(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
        bindings: &[RegisterValue],
    ) -> Result<MemoryOperand<'memory>, BuildError> {
        self.memory_operand(address, T::BYTES, Intent::Write, bindings)
    }

    /// Checks and modifies a complete memory operand. Inputs must be captured
    /// before entry; the returned value belongs to this successful update.
    pub(crate) fn modify_memory<T: MemoryInt>(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
        update: OperandUpdate<T>,
    ) -> Result<Val<T>, BuildError> {
        let target = self.prepare_memory_write::<T>(address, &[])?;
        if self.locked {
            target.atomic_update(self, &update)
        } else {
            let previous = target.read(self, 0)?;
            target.write(self, 0, update.apply(&previous))?;
            Ok(previous)
        }
    }

    /// Resolves an address once and checks every field before any transfer.
    pub(super) fn memory_operand(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
        bytes: u32,
        intent: Intent,
        bindings: &[RegisterValue],
    ) -> Result<MemoryOperand<'memory>, BuildError> {
        let offset = address::resolve(&mut self.body, &mut self.state, address.offset, bindings)?;
        let memory = self.memory.expect("a memory operand declares guest memory");
        let access = self.checked(memory, &address.segment, &offset, bytes, intent)?;
        Ok(MemoryOperand {
            memory,
            access,
            offset,
            segment: address.segment,
        })
    }
}
