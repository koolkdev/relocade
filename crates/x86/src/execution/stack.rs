//! Stack transfers keep the entry state intact until every access is guarded.

use wasm86_compiler::{BuildError, Val, I32};

use crate::{
    address::RegisterValue,
    instruction::Location,
    memory::Intent,
    register::{Gpr32, RegisterType},
    segment::Segment,
};

use super::ExecutionBuilder;

/// A guarded stack read whose pointer change has not been committed.
pub(crate) struct StackPop<T: RegisterType> {
    value: Val<T>,
    pointer: StackPointer,
    slot_bytes: u32,
}

impl<T: RegisterType> StackPop<T> {
    pub(crate) fn value(&self) -> &Val<T> {
        &self.value
    }

    /// Publishes the pointer change after the caller's remaining fault checks.
    /// Extra discarded bytes are not read or checked against the stack limit.
    pub(crate) fn commit(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        discard_bytes: impl Into<Val<I32>>,
    ) -> Result<Val<T>, BuildError> {
        execution.state.write_register(
            &mut execution.body,
            Gpr32::Esp,
            self.pointer
                .advance(discard_bytes.into().add(self.slot_bytes))
                .esp,
        )?;
        Ok(self.value)
    }
}

/// SS.B wraps pointer arithmetic independently of the transferred value width.
/// The full ESP value is retained for POP destinations with 32-bit addressing.
struct StackPointer {
    esp: Val<I32>,
    mask: Val<I32>,
}

impl StackPointer {
    fn offset(&self) -> Val<I32> {
        self.esp.and(&self.mask)
    }

    fn advance(&self, bytes: impl Into<Val<I32>>) -> Self {
        let esp = self
            .esp
            .and(self.mask.xor(u32::MAX))
            .or(self.esp.add(bytes).and(&self.mask));
        Self {
            esp,
            mask: self.mask.clone(),
        }
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

    /// Reserves a slot independently of the bytes transferred by the value.
    /// Segment pushes write a selector word even when reserving a dword slot.
    pub(crate) fn push<T: RegisterType>(
        &mut self,
        value: impl Into<Val<T>>,
        slot_bytes: u32,
    ) -> Result<(), BuildError> {
        let value = self.body.value(value)?;
        let pointer = self.stack_pointer()?.advance(-(slot_bytes as i32));
        let memory = self
            .memory
            .expect("a stack instruction declares guest memory");
        let access = self.checked(
            memory,
            &Segment::Ss.into(),
            &pointer.offset(),
            T::BYTES,
            Intent::Write,
        )?;
        memory.write(&mut self.body, &access, 0, &value)?;
        self.state
            .write_register(&mut self.body, Gpr32::Esp, pointer.esp)
    }

    pub(crate) fn pop<T: RegisterType>(
        &mut self,
        destination: Location<impl Into<Val<I32>>>,
    ) -> Result<(), BuildError> {
        let popped = self.read_stack::<T>(T::BYTES)?;
        // Address reads see next ESP while the fault state still holds entry ESP.
        let target = self.prepare_write::<T>(
            destination,
            &[RegisterValue {
                register: Gpr32::Esp,
                value: popped.pointer.advance(popped.slot_bytes).esp,
            }],
        )?;
        // POP ESP overwrites the increment; POP SP preserves its upper word.
        let value = popped.commit(self, 0)?;
        self.write_target(target, value)
    }

    /// Reads only T's bytes and saves the slot adjustment and current SS.B.
    /// Pointer commitment never needs to reread stack attributes.
    pub(crate) fn read_stack<T: RegisterType>(
        &mut self,
        slot_bytes: u32,
    ) -> Result<StackPop<T>, BuildError> {
        let pointer = self.stack_pointer()?;
        let memory = self
            .memory
            .expect("a stack instruction declares guest memory");
        let access = self.checked(
            memory,
            &Segment::Ss.into(),
            &pointer.offset(),
            T::BYTES,
            Intent::Read,
        )?;
        let value = memory.read::<T>(&mut self.body, &access, 0)?;
        Ok(StackPop {
            value,
            pointer,
            slot_bytes,
        })
    }
}
