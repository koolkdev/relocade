//! Stack transfers keep the entry state intact until every access is guarded.

use wasm86_compiler::{AtLeast, BuildError, Val, I32};

use crate::{
    address::RegisterValue,
    instruction::{Location, Operand},
    memory::Intent,
    register::{Gpr32, RegisterType},
};

use super::ExecutionBuilder;

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn push<T: RegisterType>(
        &mut self,
        source: Operand<impl Into<Val<I32>>>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        // The source, including ESP or an ESP-based address, observes entry ESP.
        let value = self.read::<T>(source)?;
        let esp = self.state.read_register(&mut self.body, Gpr32::Esp)?;
        let next_esp = esp.sub(T::BYTES);
        let memory = self
            .memory
            .expect("a stack instruction declares guest memory");
        let access = self.checked::<T>(memory, &next_esp, Intent::Write)?;
        memory.write(&mut self.body, &access, &value)?;
        self.state
            .write_register(&mut self.body, Gpr32::Esp, next_esp)
    }

    pub(crate) fn pop<T: RegisterType>(
        &mut self,
        destination: Location<impl Into<Val<I32>>>,
    ) -> Result<(), BuildError> {
        let esp = self.state.read_register(&mut self.body, Gpr32::Esp)?;
        let memory = self
            .memory
            .expect("a stack instruction declares guest memory");
        let access = self.checked::<T>(memory, &esp, Intent::Read)?;
        let value = memory.read(&mut self.body, &access)?;
        let next_esp = esp.add(T::BYTES);
        // Address reads see next ESP while the fault state still holds entry ESP.
        let target = self.prepare_write::<T>(
            destination,
            &[RegisterValue {
                register: Gpr32::Esp,
                value: next_esp.clone(),
            }],
        )?;
        // POP ESP overwrites the increment; POP SP preserves its upper word.
        self.state
            .write_register(&mut self.body, Gpr32::Esp, next_esp)?;
        self.write_target(target, value)
    }
}
