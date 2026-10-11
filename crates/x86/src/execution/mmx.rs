//! MMX admission precedes operands; environment effects follow successful execution.

use wasm86_compiler::{BuildError, Val, I64};

use super::ExecutionBuilder;
use crate::register::RegisterCode;

impl ExecutionBuilder<'_, '_> {
    /// The body must finish its guest fault checks before any architectural writes.
    pub(crate) fn execute_mmx(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        self.check_x87_exception()?;
        body(self)?;
        self.x87().complete_mmx()
    }

    pub(crate) fn read_mmx(&mut self, register: RegisterCode) -> Result<Val<I64>, BuildError> {
        self.x87().read_mmx(register)
    }

    pub(crate) fn write_mmx(
        &mut self,
        register: RegisterCode,
        value: Val<I64>,
    ) -> Result<(), BuildError> {
        self.x87().write_mmx(register, value)
    }

    pub(crate) fn empty_mmx(&mut self) -> Result<(), BuildError> {
        self.check_x87_exception()?;
        self.x87().empty_mmx()
    }
}
