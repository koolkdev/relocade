mod access;
mod cpu;
pub(super) mod exit;
mod flags;
mod layout;
#[cfg(test)]
mod observation;
mod x87;

pub(super) use cpu::Cpu;
pub use layout::{
    CpuState, FlagBytes, Registers, Segments, StoredFlags, StoredSegment, StoredStatusSource,
    StoredX87, StoredX87Register,
};
#[cfg(test)]
pub(crate) use observation::compile_flag_observer;

use access::{cpu_load, cpu_store, register_location};
use wasm86_compiler::{BuildError, FunctionBuilder, Val, I1, I16, I32};

use crate::{
    exception::Exception,
    flags::{Condition, Flag, FlagChange},
    register::{Register, RegisterType},
    segment::{SegmentSelection, SegmentValues},
    ssa::Environment,
};

#[derive(Clone)]
pub(super) struct State<'cpu> {
    cpu: &'cpu Cpu,
    registers: Environment,
    flags: flags::FlagState,
    pub(super) x87: x87::X87State,
}

impl<'cpu> State<'cpu> {
    pub(super) fn new(cpu: &'cpu Cpu) -> Self {
        Self {
            cpu,
            registers: Environment::new(cpu.memory()),
            flags: flags::FlagState::new(cpu.memory()),
            x87: x87::X87State::new(cpu.memory()),
        }
    }

    /// Reads the visible selector even when its loaded cache is unusable.
    pub(crate) fn read_segment_selector(
        &self,
        body: &mut FunctionBuilder<'_>,
        segment: &SegmentSelection,
    ) -> Result<Val<I16>, BuildError> {
        Ok(self.cpu.read_segment(body, segment)?.selector)
    }

    /// Commits a resolved cache after all instruction guards. Only completion
    /// and dispatch may follow when this breaks the entry's segment assumptions.
    pub(crate) fn write_segment(
        &self,
        body: &mut FunctionBuilder<'_>,
        segment: &SegmentSelection,
        values: &SegmentValues,
    ) -> Result<(), BuildError> {
        self.cpu.write_segment(body, segment, values)
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

    /// Reads logical flags in request order. Repeated flags share their current value.
    pub(crate) fn read_flags<const N: usize>(
        &mut self,
        body: &mut FunctionBuilder<'_>,
        flags: [Flag; N],
    ) -> Result<[Val<I1>; N], BuildError> {
        self.flags.read_flags(body, self.cpu, flags)
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

    /// Publishes current definitions and terminates at the supplied faulting EIP.
    /// Callers define only effects that are permitted to survive this fault.
    pub(super) fn fault(
        &self,
        mut body: FunctionBuilder<'_>,
        restart_eip: impl Into<Val<I32>>,
        completed: u32,
        exception: Exception<Val<I32>>,
    ) -> Result<(), BuildError> {
        self.publish(&mut body, restart_eip, completed)?;
        exit::exception(body, exception)
    }

    /// Publishes current state on a terminating path, including any permitted
    /// partial progress in an unretired instruction.
    /// Indexed accesses may already have synchronized register definitions to backing.
    /// Later definitions do not change an earlier authored exit; this does not
    /// restore an older state after partially executing a new instruction.
    pub(super) fn publish(
        &self,
        body: &mut FunctionBuilder<'_>,
        next_eip: impl Into<Val<I32>>,
        completed: u32,
    ) -> Result<(), BuildError> {
        self.flags.publish(body, self.cpu)?;
        self.x87.publish(body)?;
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
