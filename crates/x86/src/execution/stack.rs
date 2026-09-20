//! Stack frames separate capacity checks, memory transfers and pointer commitment.

mod frame;
mod registers;

use std::marker::PhantomData;

use wasm86_compiler::{BuildError, Val, I32};

use crate::{
    address::RegisterValue,
    instruction::Location,
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

    fn next_pointer(&self) -> StackPointer {
        self.pointer.advance(self.adjustment)
    }

    /// Extra discarded bytes are neither accessed nor checked against SS.limit.
    pub(crate) fn commit(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        discard_bytes: impl Into<Val<I32>>,
    ) -> Result<(), BuildError> {
        execution.state.write_register(
            &mut execution.body,
            Gpr32::Esp,
            self.pointer
                .advance(discard_bytes.into().add(self.adjustment))
                .esp,
        )
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

/// SS.B wraps pointer arithmetic independently of the transferred value width.
/// For a wrapped 16-bit-stack POP to memory, wasm86 uses the incremented ESP
/// with its upper word preserved. That destination is processor-family-specific;
/// retaining the full ESP also supplies 32-bit destination addressing.
#[derive(Clone)]
struct StackPointer {
    esp: Val<I32>,
    mask: Val<I32>,
}

impl StackPointer {
    fn offset(&self) -> Val<I32> {
        self.esp.and(&self.mask)
    }

    fn with_offset(&self, offset: impl Into<Val<I32>>) -> Self {
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
}

impl ExecutionBuilder<'_, '_> {
    fn stack_pointer(&mut self) -> Result<StackPointer, BuildError> {
        let esp = self.state.read_register(&mut self.body, Gpr32::Esp)?;
        let mask = self
            .segments
            .is_segment_big(&mut self.body, Segment::Ss)?
            .select(u32::MAX, 0xffffu32);
        Ok(StackPointer { esp, mask })
    }

    /// Reserves bytes below the entry pointer and checks capacity at the new SP.
    pub(crate) fn push_frame(
        &mut self,
        slot_bytes: u32,
        checked_bytes: u32,
    ) -> Result<StackFrame, BuildError> {
        let pointer = self.stack_pointer()?;
        self.push_frame_at(pointer, slot_bytes, checked_bytes)
    }

    fn push_frame_at(
        &mut self,
        pointer: StackPointer,
        slot_bytes: u32,
        checked_bytes: u32,
    ) -> Result<StackFrame, BuildError> {
        let adjustment = -(slot_bytes as i32);
        let offset = pointer.advance(adjustment).offset();
        let intent = Intent::Write;
        let linear = self.translate(&Segment::Ss.into(), &offset, checked_bytes, intent)?;
        Ok(StackFrame {
            linear,
            checked_bytes,
            intent,
            pointer,
            adjustment,
        })
    }

    /// Checks capacity at entry SP and saves an adjustment using the entry SS.B.
    /// Fields within the frame are consecutive; only pointer arithmetic wraps.
    pub(crate) fn pop_frame(
        &mut self,
        slot_bytes: u32,
        checked_bytes: u32,
    ) -> Result<StackFrame, BuildError> {
        let pointer = self.stack_pointer()?;
        self.pop_frame_at(pointer, slot_bytes, checked_bytes)
    }

    fn pop_frame_at(
        &mut self,
        pointer: StackPointer,
        slot_bytes: u32,
        checked_bytes: u32,
    ) -> Result<StackFrame, BuildError> {
        let offset = pointer.offset();
        let intent = Intent::Read;
        let linear = self.translate(&Segment::Ss.into(), &offset, checked_bytes, intent)?;
        Ok(StackFrame {
            linear,
            checked_bytes,
            intent,
            pointer,
            adjustment: slot_bytes as i32,
        })
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

    pub(crate) fn pop<T: RegisterType>(
        &mut self,
        destination: Location<impl Into<Val<I32>>>,
    ) -> Result<(), BuildError> {
        let frame = self.pop_frame(T::BYTES, T::BYTES)?;
        let value = frame.field::<T>(self, 0)?.read(self)?;
        // Address reads see next ESP while the fault state still holds entry ESP.
        let target = self.prepare_write::<T>(
            destination,
            &[RegisterValue {
                register: Gpr32::Esp,
                value: frame.next_pointer().esp,
            }],
        )?;
        // POP ESP overwrites the increment; POP SP preserves its upper word.
        frame.commit(self, 0)?;
        self.write_target(target, value)
    }
}
