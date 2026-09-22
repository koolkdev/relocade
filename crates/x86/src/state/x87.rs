//! The x87 environment owns control changes and publication at guest exits.

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, Val, I1, I16};

use crate::ssa::Environment;

use super::access::cpu_location;

#[derive(Clone)]
pub(crate) struct X87State {
    environment: Environment,
}

impl X87State {
    pub(crate) fn new(memory: Mem) -> Self {
        Self {
            environment: Environment::new(memory),
        }
    }

    pub(crate) fn control_word(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.environment.read(body, cpu_location!(x87.control_word))
    }

    pub(crate) fn status_word(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I16>, BuildError> {
        self.environment.read(body, cpu_location!(x87.status_word))
    }

    pub(crate) fn pending_exception(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<Val<I1>, BuildError> {
        Ok(self.status_word(body)?.and(0x0080).ne(0))
    }

    pub(crate) fn initialize(&mut self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        // FNINIT marks the stack empty without changing register payloads.
        self.environment
            .define(body, cpu_location!(x87.control_word), 0x037f)?;
        self.environment
            .define(body, cpu_location!(x87.status_word), 0)?;
        self.environment
            .define(body, cpu_location!(x87.tag_word), 0xffff)?;
        self.environment
            .define(body, cpu_location!(x87.opcode), 0)?;
        self.environment
            .define(body, cpu_location!(x87.instruction_offset), 0)?;
        self.environment
            .define(body, cpu_location!(x87.data_offset), 0)?;
        self.environment
            .define(body, cpu_location!(x87.instruction_selector), 0)?;
        self.environment
            .define(body, cpu_location!(x87.data_selector), 0)
    }

    pub(crate) fn clear_exceptions(
        &mut self,
        body: &mut FunctionBuilder<'_>,
    ) -> Result<(), BuildError> {
        let status = self.status_word(body)?;
        // C0/C1/C2/C3 are undefined for FNCLEX; retain them and the unchanged TOP.
        self.environment
            .define(body, cpu_location!(x87.status_word), status.and(0x7f00))
    }

    pub(crate) fn load_control_word(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        control: Val<I16>,
    ) -> Result<(), BuildError> {
        let status = self.status_word(body)?;
        let pending = status
            .and(0x003f)
            .and(control.xor(0xffff))
            .ne(0)
            .unsigned()
            .extend::<I16>();
        let status = status.and(0x7f7f).or(pending.shl(7)).or(pending.shl(15));
        self.environment
            .define(body, cpu_location!(x87.control_word), control)?;
        self.environment
            .define(body, cpu_location!(x87.status_word), status)
    }

    pub(crate) fn publish(&self, body: &mut FunctionBuilder<'_>) -> Result<(), BuildError> {
        self.environment.publish(body)
    }
}
