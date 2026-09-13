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
    next_esp: Val<I32>,
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
            self.next_esp.add(discard_bytes),
        )?;
        Ok(self.value)
    }
}

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn push<T: RegisterType>(
        &mut self,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        let value = self.body.value(value)?;
        let esp = self.state.read_register(&mut self.body, Gpr32::Esp)?;
        let next_esp = esp.sub(T::BYTES);
        let memory = self
            .memory
            .expect("a stack instruction declares guest memory");
        let access = self.checked::<T>(memory, &Segment::Ss.into(), &next_esp, Intent::Write)?;
        memory.write(&mut self.body, &access, &value)?;
        self.state
            .write_register(&mut self.body, Gpr32::Esp, next_esp)
    }

    pub(crate) fn pop<T: RegisterType>(
        &mut self,
        destination: Location<impl Into<Val<I32>>>,
    ) -> Result<(), BuildError> {
        let popped = self.read_stack::<T>()?;
        // Address reads see next ESP while the fault state still holds entry ESP.
        let target = self.prepare_write::<T>(
            destination,
            &[RegisterValue {
                register: Gpr32::Esp,
                value: popped.next_esp.clone(),
            }],
        )?;
        // POP ESP overwrites the increment; POP SP preserves its upper word.
        let value = popped.commit(self, 0)?;
        self.write_target(target, value)
    }

    pub(crate) fn read_stack<T: RegisterType>(&mut self) -> Result<StackPop<T>, BuildError> {
        let esp = self.state.read_register(&mut self.body, Gpr32::Esp)?;
        let memory = self
            .memory
            .expect("a stack instruction declares guest memory");
        let access = self.checked::<T>(memory, &Segment::Ss.into(), &esp, Intent::Read)?;
        let value = memory.read(&mut self.body, &access)?;
        Ok(StackPop {
            value,
            next_esp: esp.add(T::BYTES),
        })
    }
}
