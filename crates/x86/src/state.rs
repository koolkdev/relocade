mod access;
mod cpu;
pub(super) mod exit;
mod flags;
mod layout;
#[cfg(test)]
mod observation;

pub(super) use cpu::Cpu;
pub use layout::{CpuState, FlagBytes, Registers, StoredFlags, StoredStatusSource};
#[cfg(test)]
pub(crate) use observation::compile_flag_observer;

use access::{cpu_load, cpu_store, register_location};
use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I32};

use crate::{
    flags::{Condition, Flag, FlagChange},
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
            flags: flags::FlagState::new(cpu.memory()),
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
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        self.registers
            .define(body, register_location(register.into()), value)
    }

    pub(crate) fn read_flag(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        flag: Flag,
    ) -> Result<Val<I1>, BuildError> {
        self.flags.read(body, self.cpu, flag)
    }

    pub(crate) fn write_flag(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        flag: Flag,
        value: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        self.write_flags(body, FlagChange::partial([(flag, value.into())]))
    }

    /// Defines a masked, optionally conditional change. Omitted flags retain
    /// their current values; backing bytes are written when state is published.
    pub(crate) fn write_flags(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        change: impl Into<FlagChange>,
    ) -> Result<(), BuildError> {
        self.flags.apply(body, change.into())
    }

    pub(crate) fn condition(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        self.flags.condition(body, self.cpu, condition)
    }

    /// Publishes current completed instructions on a terminating path. Indexed
    /// accesses may already have synchronized register definitions to backing.
    /// Later definitions do not change an earlier authored exit; this does not
    /// restore an older state after partially executing a new instruction.
    pub(super) fn publish(
        &self,
        body: &mut FunctionBuilder<'_>,
        next_eip: impl Into<Val<I32>>,
        completed: u32,
    ) -> Result<(), BuildError> {
        self.flags.publish(body, self.cpu)?;
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
