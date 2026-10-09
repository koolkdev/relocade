//! Checked memory operands support widths independently of register views.

use wasm86_compiler::{BuildError, MemoryInt, Val, I32};

use super::ExecutionBuilder;
use crate::{
    address::{self, MemoryAddress, RegisterValue},
    alu::OperandUpdate,
    memory::{Access, Intent, Memory},
    segment::SegmentSelection,
};

/// A resolved operand whose complete span passed its architectural access checks.
/// Fields share that proof, so a later field cannot fault after an earlier write.
pub(crate) struct MemoryOperand<'memory> {
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

    pub(crate) fn read<T: MemoryInt>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        self.memory
            .read::<T>(&mut execution.body, &self.access, offset)
    }

    pub(crate) fn write<T: MemoryInt>(
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
    pub(super) fn checked(
        &mut self,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<Access, BuildError> {
        let linear = self.translate(segment, offset, bytes, intent)?;
        self.resolve_access(&linear, bytes, intent)
    }

    pub(super) fn translate(
        &mut self,
        segment: &SegmentSelection,
        offset: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<Val<I32>, BuildError> {
        self.segments.translate(
            &mut self.body,
            segment,
            offset,
            bytes,
            intent,
            |mut fault_body, exception| {
                self.runtime
                    .publish_work(&mut fault_body, self.work.as_ref())?;
                self.state
                    .fault(fault_body, &self.eip, self.completed, exception)
            },
        )
    }

    pub(super) fn resolve_access(
        &mut self,
        linear: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<Access, BuildError> {
        let memory = self
            .memory
            .as_ref()
            .expect("a memory access declares guest memory")
            .memory();
        if memory.tracks_code() && self.can_specialize && !memory.has_stable_mappings() {
            let direct =
                memory.check_direct_access(&mut self.body, linear, bytes, intent, None, None)?;
            self.specialize_on(direct.unavailable.eq(false))?;
            return Ok(Access {
                linear: linear.clone(),
                physical: direct.physical,
                denied: false.into(),
                unavailable: false.into(),
                watched: false.into(),
                intent,
                constant_bytes: Some(bytes),
            });
        }
        let mut access = self.memory.as_mut().unwrap().resolve(
            &mut self.body,
            linear,
            bytes,
            intent,
            |mut fault_body, exception| {
                self.runtime
                    .publish_work(&mut fault_body, self.work.as_ref())?;
                self.state
                    .fault(fault_body, &self.eip, self.completed, exception)
            },
        )?;
        if memory.tracks_code() && matches!(intent, Intent::Write) && self.can_specialize {
            self.specialize_on(access.watched.eq(false))?;
            access.watched = false.into();
        }
        Ok(access)
    }

    /// Rare operations with interleaved architectural effects cannot restart
    /// at a later protected memory access. Keep their precise checked semantics.
    pub(crate) fn interpret_tracked_memory(&mut self) -> Result<(), BuildError> {
        if self
            .memory
            .as_ref()
            .is_some_and(|memory| memory.memory().tracks_code())
        {
            self.specialize(|jit| jit.specialize_on(false))?;
        }
        Ok(())
    }

    /// Reads a linear address without applying a data segment or address-size wrap.
    /// System-table accesses still use the profile's ordinary memory routing.
    pub(crate) fn read_linear_memory<T: MemoryInt>(
        &mut self,
        linear: impl Into<Val<I32>>,
    ) -> Result<Val<T>, BuildError> {
        let access = self.resolve_access(&linear.into(), T::BYTES, Intent::Read)?;
        self.memory
            .as_ref()
            .expect("a memory read declares guest memory")
            .memory()
            .read(&mut self.body, &access, 0)
    }

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
    pub(crate) fn memory_operand(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
        bytes: u32,
        intent: Intent,
        bindings: &[RegisterValue],
    ) -> Result<MemoryOperand<'memory>, BuildError> {
        let offset = address::resolve(&mut self.body, &mut self.state, address.offset, bindings)?;
        let memory = self
            .memory
            .as_ref()
            .expect("a memory operand declares guest memory")
            .memory();
        let access = self.checked(&address.segment, &offset, bytes, intent)?;
        Ok(MemoryOperand {
            memory,
            access,
            offset,
            segment: address.segment,
        })
    }
}
