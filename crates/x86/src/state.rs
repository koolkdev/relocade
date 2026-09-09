mod access;
mod cpu;
pub(super) mod exit;
mod flags;
mod layout;

pub(super) use cpu::Cpu;
pub use layout::{CpuState, Registers, StatusFlags, StoredFlags};

use access::{cpu_load, cpu_store, register_location};
use wasm86_compiler::{BuildError, FunctionBuilder, IntoOp, Val, I32};

use crate::{
    register::{Register, RegisterType},
    ssa::Environment,
};

pub(super) struct State<'cpu> {
    cpu: &'cpu Cpu,
    registers: Environment,
    flags: flags::FlagState,
}

impl<'cpu> State<'cpu> {
    pub(super) fn new(cpu: &'cpu Cpu) -> Self {
        Self {
            cpu,
            registers: Environment::new(cpu.memory()),
            flags: flags::FlagState::default(),
        }
    }

    pub(super) fn read_register<T: RegisterType>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        register: impl Into<Register<T>>,
    ) -> Result<Val<T>, BuildError> {
        self.registers
            .read(body, register_location(register.into()))
    }

    pub(super) fn write_register<T: RegisterType>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        register: impl Into<Register<T>>,
        value: impl IntoOp<T>,
    ) -> Result<(), BuildError> {
        self.registers
            .define(body, register_location(register.into()), value)
    }

    /// Publishes current completed instructions on a terminating path. Indexed
    /// accesses may already have synchronized register definitions to backing.
    /// Later definitions do not change an earlier authored exit; this does not
    /// restore an older state after partially executing a new instruction.
    pub(super) fn publish(
        &self,
        body: &mut FunctionBuilder<'_>,
        next_eip: impl IntoOp<I32>,
        completed: u32,
    ) -> Result<(), BuildError> {
        self.publish_flags(body)?;
        self.registers.publish(body)?;
        cpu_store!(body, self.cpu.memory(), eip, next_eip)?;
        if completed != 0 {
            let count = cpu_load!(body, self.cpu.memory(), instruction_count)?;
            cpu_store!(
                body,
                self.cpu.memory(),
                instruction_count,
                count.add(completed)
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
