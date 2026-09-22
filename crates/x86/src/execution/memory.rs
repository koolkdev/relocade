//! Checked memory operands support widths independently of register views.

use std::marker::PhantomData;

use wasm86_compiler::{BuildError, MemoryInt, Val, I32};

use super::ExecutionBuilder;
use crate::{
    address::{self, MemoryAddress, RegisterValue},
    alu::OperandUpdate,
    memory::{Access, Intent, Memory},
};

pub(super) struct MemoryWriteTarget<'memory, T: MemoryInt> {
    memory: &'memory Memory,
    access: Access,
    width: PhantomData<T>,
}

impl<T: MemoryInt> MemoryWriteTarget<'_, T> {
    pub(super) fn read(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<Val<T>, BuildError> {
        self.memory.read::<T>(&mut execution.body, &self.access, 0)
    }

    pub(super) fn write(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        let value = execution.body.value(value)?;
        self.memory
            .write(&mut execution.body, &self.access, 0, &value)
    }

    pub(super) fn atomic_update(
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
        let (memory, access) = self.memory_operand(address, T::BYTES, Intent::Read, &[])?;
        memory.read::<T>(&mut self.body, &access, 0)
    }

    pub(super) fn prepare_memory_write<T: MemoryInt>(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
        bindings: &[RegisterValue],
    ) -> Result<MemoryWriteTarget<'memory, T>, BuildError> {
        let (memory, access) = self.memory_operand(address, T::BYTES, Intent::Write, bindings)?;
        Ok(MemoryWriteTarget {
            memory,
            access,
            width: PhantomData,
        })
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
            let previous = target.read(self)?;
            target.write(self, update.apply(&previous))?;
            Ok(previous)
        }
    }

    fn memory_operand(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
        bytes: u32,
        intent: Intent,
        bindings: &[RegisterValue],
    ) -> Result<(&'memory Memory, Access), BuildError> {
        let offset = address::resolve(&mut self.body, &mut self.state, address.offset, bindings)?;
        let memory = self.memory.expect("a memory operand declares guest memory");
        let access = self.checked(memory, &address.segment, &offset, bytes, intent)?;
        Ok((memory, access))
    }
}
