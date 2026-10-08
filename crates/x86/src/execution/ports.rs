//! Typed port operands use the runtime's synchronous device interface.

use super::*;
use wasm86_compiler::{AtLeast, MemoryInt};

impl ExecutionBuilder<'_, '_> {
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
