//! Stack frames separate capacity checks, memory transfers and pointer commitment.

mod frame;

use std::marker::PhantomData;

use wasm86_compiler::{BuildError, Val, I32};

use crate::{
    memory::{Access, Intent, Memory},
    register::{Gpr32, RegisterType},
    segment::Segment,
};

use super::ExecutionBuilder;

/// A frame that fits SS, with its prospective pointer change still uncommitted.
/// Paging may follow other architectural checks, such as a far CALL target limit.
pub(crate) struct StackFrame {
    linear: Val<I32>,
    checked_bytes: u32,
    intent: Intent,
    pointer: StackPointer,
    adjustment: i32,
}

impl StackFrame {
    /// Proves a typed field's page access inside the segment-checked frame.
    /// Callers prove every write field before storing any of them; field order
    /// determines which page fault wins when several fields are inaccessible.
    pub(crate) fn field<'module, T: RegisterType>(
        &self,
        execution: &mut ExecutionBuilder<'_, 'module>,
        offset: u32,
    ) -> Result<StackField<'module, T>, BuildError> {
        assert!(offset <= self.checked_bytes && T::BYTES <= self.checked_bytes - offset);
        let memory = execution
            .memory
            .expect("a stack instruction declares guest memory");
        let access =
            execution.resolve_access(memory, &self.linear.add(offset), T::BYTES, self.intent)?;
        Ok(StackField {
            memory,
            access,
            marker: PhantomData,
        })
    }

    pub(crate) fn next_pointer(&self) -> StackPointer {
        self.pointer.advance(self.adjustment)
    }

    /// Extra discarded bytes are neither accessed nor checked against SS.limit.
    pub(crate) fn commit(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        discard_bytes: impl Into<Val<I32>>,
    ) -> Result<(), BuildError> {
        self.pointer
            .advance(discard_bytes.into().add(self.adjustment))
            .commit(execution)
    }
}

/// A typed field with a complete access proof. Slot padding is not transferred.
pub(crate) struct StackField<'module, T: RegisterType> {
    memory: &'module Memory,
    access: Access,
    marker: PhantomData<T>,
}

impl<T: RegisterType> StackField<'_, T> {
    pub(crate) fn read(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
    ) -> Result<Val<T>, BuildError> {
        self.memory.read(&mut execution.body, &self.access, 0)
    }

    pub(crate) fn write(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        value: &Val<T>,
    ) -> Result<(), BuildError> {
        self.memory
            .write(&mut execution.body, &self.access, 0, value)
    }
}

/// A prospective ESP value whose arithmetic uses the captured SS.B width.
/// Deriving pointers and checking frames does not update architectural ESP.
#[derive(Clone)]
pub(crate) struct StackPointer {
    esp: Val<I32>,
    mask: Val<I32>,
}

impl StackPointer {
    /// Includes the preserved upper word when SS.B selects a 16-bit stack.
    pub(crate) fn value(&self) -> Val<I32> {
        self.esp.clone()
    }

    fn offset(&self) -> Val<I32> {
        self.esp.and(&self.mask)
    }

    /// Replaces the active SP/ESP bits, preserving the entry value's other bits.
    pub(crate) fn with_offset(&self, offset: impl Into<Val<I32>>) -> Self {
        let esp = self
            .esp
            .and(self.mask.xor(u32::MAX))
            .or(offset.into().and(&self.mask));
        Self {
            esp,
            mask: self.mask.clone(),
        }
    }

    fn advance(&self, bytes: impl Into<Val<I32>>) -> Self {
        self.with_offset(self.esp.add(bytes))
    }

    pub(crate) fn commit(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
        execution
            .state
            .write_register(&mut execution.body, Gpr32::Esp, self.esp)
    }

    /// Reserves bytes below this pointer and checks capacity at the new SP.
    pub(crate) fn push_frame(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        slot_bytes: u32,
        checked_bytes: u32,
    ) -> Result<StackFrame, BuildError> {
        let adjustment = -(slot_bytes as i32);
        let offset = self.advance(adjustment).offset();
        let intent = Intent::Write;
        let linear = execution.translate(&Segment::Ss.into(), &offset, checked_bytes, intent)?;
        Ok(StackFrame {
            linear,
            checked_bytes,
            intent,
            pointer: self,
            adjustment,
        })
    }

    /// Checks capacity at this SP and saves an adjustment using the captured SS.B.
    /// Fields within the frame are consecutive; only pointer arithmetic wraps.
    pub(crate) fn pop_frame(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        slot_bytes: u32,
        checked_bytes: u32,
    ) -> Result<StackFrame, BuildError> {
        let offset = self.offset();
        let intent = Intent::Read;
        let linear = execution.translate(&Segment::Ss.into(), &offset, checked_bytes, intent)?;
        Ok(StackFrame {
            linear,
            checked_bytes,
            intent,
            pointer: self,
            adjustment: slot_bytes as i32,
        })
    }
}

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn stack_pointer(&mut self) -> Result<StackPointer, BuildError> {
        let esp = self.state.read_register(&mut self.body, Gpr32::Esp)?;
        let mask = self
            .segments
            .is_segment_big(&mut self.body, Segment::Ss)?
            .select(u32::MAX, 0xffffu32);
        Ok(StackPointer { esp, mask })
    }

    pub(crate) fn push_frame(
        &mut self,
        slot_bytes: u32,
        checked_bytes: u32,
    ) -> Result<StackFrame, BuildError> {
        self.stack_pointer()?
            .push_frame(self, slot_bytes, checked_bytes)
    }

    pub(crate) fn pop_frame(
        &mut self,
        slot_bytes: u32,
        checked_bytes: u32,
    ) -> Result<StackFrame, BuildError> {
        self.stack_pointer()?
            .pop_frame(self, slot_bytes, checked_bytes)
    }

    /// Segment pushes transfer a selector word even in a dword-sized slot.
    /// This follows the P6 selector-transfer policy also used by segment POP:
    /// unused slot padding is neither accessed nor checked against SS.limit.
    pub(crate) fn push<T: RegisterType>(
        &mut self,
        value: impl Into<Val<T>>,
        slot_bytes: u32,
    ) -> Result<(), BuildError> {
        let value = self.body.value(value)?;
        let frame = self.push_frame(slot_bytes, T::BYTES)?;
        frame.field::<T>(self, 0)?.write(self, &value)?;
        frame.commit(self, 0)
    }
}
