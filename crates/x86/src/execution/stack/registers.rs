//! All-register stack transfers wrap the pointer between operand-sized slots.
//!
//! Check each slot in transfer order. Later faults retain completed PUSHA stores
//! or POPA register restores, while ESP stays at its entry value. POPA checks the
//! discarded SP/ESP slot against both SS and paging without reading its value.

use wasm86_compiler::BuildError;

use crate::register::{Gpr32, Register, RegisterType};

use super::ExecutionBuilder;

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn push_all_registers<T: RegisterType>(&mut self) -> Result<(), BuildError> {
        let mut pointer = self.stack_pointer()?;
        for register in Gpr32::ALL {
            // ESP stays at its entry value until all eight pushes succeed.
            let value = self
                .state
                .read_register(&mut self.body, Register::<T>::named(register))?;
            let frame = self.push_frame_at(pointer, T::BYTES, T::BYTES)?;
            frame.field::<T>(self, 0)?.write(self, &value)?;
            pointer = frame.next_pointer();
        }
        self.state
            .write_register(&mut self.body, Gpr32::Esp, pointer.esp)
    }

    pub(crate) fn pop_all_registers<T: RegisterType>(&mut self) -> Result<(), BuildError> {
        let mut pointer = self.stack_pointer()?;
        for register in Gpr32::ALL.into_iter().rev() {
            let frame = self.pop_frame_at(pointer, T::BYTES, T::BYTES)?;
            let field = frame.field::<T>(self, 0)?;
            if register != Gpr32::Esp {
                let value = field.read(self)?;
                self.state
                    .write_register(&mut self.body, Register::<T>::named(register), value)?;
            }
            pointer = frame.next_pointer();
        }
        self.state
            .write_register(&mut self.body, Gpr32::Esp, pointer.esp)
    }
}
