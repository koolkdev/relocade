//! Checked memory operands support widths independently of register views.

use crate::memory::TransferType;
use relocade_compiler::{BuildError, MemoryInt, MemoryType, Val, VectorType, I32};

use super::ExecutionBuilder;
use crate::{
    address::{self, MemoryAddress, RegisterValue},
    alu::OperandUpdate,
    memory::{Access, Intent, Memory},
    segment::SegmentSelection,
};

/// A complete operand width and any architectural alignment requirement.
#[derive(Clone, Copy)]
pub(crate) struct OperandSpan {
    pub(super) bytes: u32,
    pub(super) alignment: u32,
}

impl OperandSpan {
    pub(crate) fn aligned(bytes: u32, alignment: u32) -> Self {
        assert!(alignment.is_power_of_two());
        Self { bytes, alignment }
    }
}

impl From<u32> for OperandSpan {
    fn from(bytes: u32) -> Self {
        Self {
            bytes,
            alignment: 1,
        }
    }
}

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

    pub(crate) fn read<T: TransferType>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        offset: u32,
    ) -> Result<Val<T>, BuildError> {
        self.memory
            .read::<T>(&mut execution.body, &self.access, offset)
    }

    pub(crate) fn write<T: TransferType>(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        offset: u32,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        let value = execution.body.value(value)?;
        self.memory
            .write(&mut execution.body, &self.access, offset, &value)
    }

    pub(super) fn atomic_update<T: MemoryInt + TransferType>(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        update: &OperandUpdate<T>,
    ) -> Result<Val<T>, BuildError> {
        self.memory
            .atomic_update(&mut execution.body, &self.access, update)
    }
}

impl<'memory> ExecutionBuilder<'_, 'memory> {
    /// Wasm's full fence supplies the ordering required by all three x86 fences.
    /// Its stronger ordering also keeps accesses in generated helpers on their
    /// respective sides of the instruction.
    pub(crate) fn memory_fence(&mut self) {
        self.body.atomic_fence();
    }

    /// Validates CLFLUSH's byte address without transferring device or RAM data.
    /// Guest backing is coherent; there is no separate guest cache to flush.
    pub(crate) fn flush_cache_line(
        &mut self,
        address: MemoryAddress<Val<I32>>,
    ) -> Result<(), BuildError> {
        self.memory_operand(address, 1, Intent::ReadOrExecute, &[])?;
        self.memory_fence();
        Ok(())
    }

    /// Checks the complete destination before writing bytes selected by mask sign
    /// bits. Masked-off bytes are untouched, and routing stays live for each write.
    pub(crate) fn write_masked_memory<V: VectorType + MemoryType>(
        &mut self,
        address: MemoryAddress<Val<I32>>,
        value: Val<V>,
        mask: Val<V>,
    ) -> Result<(), BuildError> {
        // Intel permits address and page faults even for an all-zero mask. Check
        // the complete operand before any write, as for other non-repeated stores.
        let operand = self.memory_operand(address, V::BYTES, Intent::Write, &[])?;
        operand
            .memory
            .write_masked(&mut self.body, &operand.access, &value, &mask)
    }

    /// Reads a linear address without applying a data segment or address-size wrap.
    /// System-table accesses still use the profile's ordinary memory routing.
    pub(crate) fn read_linear_memory<T: MemoryInt + TransferType>(
        &mut self,
        linear: impl Into<Val<I32>>,
    ) -> Result<Val<T>, BuildError> {
        let access = self.resolve_access(&linear.into(), T::BYTES, Intent::Read)?;
        self.accesses.memory().read(&mut self.body, &access, 0)
    }

    pub(super) fn read_memory<T: TransferType>(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
    ) -> Result<Val<T>, BuildError> {
        self.memory_operand(address, T::BYTES, Intent::Read, &[])?
            .read(self, 0)
    }

    pub(super) fn prepare_memory_write<T: MemoryType>(
        &mut self,
        address: MemoryAddress<impl Into<Val<I32>>>,
        bindings: &[RegisterValue],
    ) -> Result<MemoryOperand<'memory>, BuildError> {
        self.memory_operand(address, T::BYTES, Intent::Write, bindings)
    }

    /// Checks and modifies a complete memory operand. Inputs must be captured
    /// before entry; the returned value belongs to this successful update.
    pub(crate) fn modify_memory<T: MemoryInt + TransferType>(
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
        span: impl Into<OperandSpan>,
        intent: Intent,
        bindings: &[RegisterValue],
    ) -> Result<MemoryOperand<'memory>, BuildError> {
        let offset = address::resolve(&mut self.body, &mut self.state, address.offset, bindings)?;
        let memory = self.accesses.memory();
        let access = self.checked(&address.segment, &offset, span, intent)?;
        Ok(MemoryOperand {
            memory,
            access,
            offset,
            segment: address.segment,
        })
    }
}
