//! Typed port operands use the runtime's synchronous device interface.

use super::*;
use relocade_compiler::{AtLeast, MemoryInt};

impl ExecutionBuilder<'_, '_> {
    /// Port callbacks can remap physical memory between string elements or
    /// before an input reaches its destination. The interpreter rechecks routing.
    pub(crate) fn guard_port_string_mappings(&mut self) -> Result<(), BuildError> {
        self.if_can_fallback(|jit| jit.guard(jit.accesses.memory().has_stable_mappings()))?;
        Ok(())
    }

    pub(crate) fn read_port<T: MemoryInt>(&mut self, port: &Val<I16>) -> Result<Val<T>, BuildError>
    where
        I32: AtLeast<T>,
    {
        self.runtime.read_port(&mut self.body, port)
    }

    pub(crate) fn write_port<T: MemoryInt>(
        &mut self,
        port: &Val<I16>,
        value: &Val<T>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        self.runtime.write_port(&mut self.body, port, value)
    }
}
