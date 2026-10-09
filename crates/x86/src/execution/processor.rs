//! Fixed-width register effects of virtual processor queries.

use super::*;
use crate::register::{Gpr32, Register};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn rdtsc(&mut self) -> Result<(), BuildError> {
        let counter = self.runtime.read_timestamp_counter(&mut self.body)?;
        self.write_register_pair::<I32>(Gpr32::Edx, Gpr32::Eax, counter)
    }

    pub(crate) fn cpuid(&mut self) -> Result<(), BuildError> {
        let leaf = self
            .state
            .read_register(&mut self.body, Register::<I32>::named(Gpr32::Eax))?;
        let subleaf = self
            .state
            .read_register(&mut self.body, Register::<I32>::named(Gpr32::Ecx))?;
        let result = self.runtime.cpuid(&mut self.body, &leaf, &subleaf)?;
        for (register, value) in [
            (Gpr32::Eax, result.eax),
            (Gpr32::Ebx, result.ebx),
            (Gpr32::Ecx, result.ecx),
            (Gpr32::Edx, result.edx),
        ] {
            self.state
                .write_register(&mut self.body, Register::<I32>::named(register), value)?;
        }
        Ok(())
    }
}
