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
struct StackPop<T: RegisterType> {
    value: Val<T>,
    next_esp: Val<I32>,
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
        self.state
            .write_register(&mut self.body, Gpr32::Esp, popped.next_esp)?;
        self.write_target(target, popped.value)
    }

    /// Pops a value and discards extra bytes beyond its stack cell without reading them.
    pub(crate) fn pop_value<T: RegisterType>(
        &mut self,
        discard_bytes: impl Into<Val<I32>>,
    ) -> Result<Val<T>, BuildError> {
        let popped = self.read_stack::<T>()?;
        self.state.write_register(
            &mut self.body,
            Gpr32::Esp,
            popped.next_esp.add(discard_bytes),
        )?;
        Ok(popped.value)
    }

    fn read_stack<T: RegisterType>(&mut self) -> Result<StackPop<T>, BuildError> {
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
