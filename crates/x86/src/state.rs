mod access;
mod cpu;
pub(super) mod exit;
mod flags;
mod layout;
#[cfg(test)]
mod observation;
pub(crate) mod x87;

pub(super) use cpu::Cpu;
pub use layout::{
    CpuState, FlagBytes, Registers, Segments, StoredFlags, StoredSegment, StoredStatusSource,
    StoredX87, StoredX87Control, StoredX87Register, StoredX87Status,
};
#[cfg(test)]
pub(crate) use observation::compile_flag_observer;
pub(crate) use x87::{Arithmetic, ArithmeticSource, LoadSource, X87Access};

use access::{cpu_load, cpu_store, register_location};
use wasm86_compiler::{BlockBuilder, BuildError, Val, I1, I16, I32, I64, I8};

use crate::{
    exception::Exception,
    flags::{Condition, Flag, FlagChange},
    register::{Register, RegisterType},
    segment::{SegmentSelection, SegmentValues},
    ssa::StateFields,
};

#[derive(Clone)]
pub(super) struct State<'cpu> {
    cpu: &'cpu Cpu,
    registers: StateFields,
    flags: flags::FlagState,
    pub(super) x87: x87::X87State,
}

impl<'cpu> State<'cpu> {
    pub(super) fn new(cpu: &'cpu Cpu) -> Self {
        Self {
            cpu,
            registers: StateFields::new(cpu.memory()),
            flags: flags::FlagState::new(cpu.memory()),
            x87: x87::X87State::new(cpu.memory()),
        }
    }

    /// Reads the visible selector even when its loaded cache is unusable.
    pub(crate) fn read_segment_selector(
        &self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
    ) -> Result<Val<I16>, BuildError> {
        Ok(self.cpu.read_segment(body, segment)?.selector)
    }

    /// Commits a resolved cache after all instruction guards. Only completion
    /// and dispatch may follow when this breaks the entry's segment assumptions.
    pub(crate) fn write_segment(
        &self,
        body: &mut BlockBuilder<'_>,
        segment: &SegmentSelection,
        values: &SegmentValues,
    ) -> Result<(), BuildError> {
        self.cpu.write_segment(body, segment, values)
    }

    pub(super) fn read_register<T: RegisterType>(
        &mut self,
        body: &mut BlockBuilder<'_>,
        register: impl Into<Register<T>>,
    ) -> Result<Val<T>, BuildError> {
        self.registers
            .read(body, register_location(register.into()))
    }

    pub(super) fn write_register<T: RegisterType>(
        &mut self,
        body: &mut BlockBuilder<'_>,
        register: impl Into<Register<T>>,
        value: impl Into<Val<T>>,
    ) -> Result<(), BuildError> {
        self.registers
            .define(body, register_location(register.into()), value)
    }

    pub(crate) fn read_flag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        flag: Flag,
    ) -> Result<Val<I1>, BuildError> {
        self.flags.read(body, self.cpu, flag)
    }

    /// Reads logical flags in request order. Repeated flags share their current value.
    pub(crate) fn read_flags<const N: usize>(
        &mut self,
        body: &mut BlockBuilder<'_>,
        flags: [Flag; N],
    ) -> Result<[Val<I1>; N], BuildError> {
        self.flags.read_flags(body, self.cpu, flags)
    }

    pub(crate) fn write_flag(
        &mut self,
        body: &mut BlockBuilder<'_>,
        flag: Flag,
        value: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        self.write_flags(body, FlagChange::partial([(flag, value.into())]))
    }

    /// Defines a masked, optionally conditional change. Omitted flags retain
    /// their current values; backing bytes are written when state is published.
    pub(crate) fn write_flags(
        &mut self,
        body: &mut BlockBuilder<'_>,
        change: impl Into<FlagChange>,
    ) -> Result<(), BuildError> {
        self.flags.apply(body, change.into())
    }

    /// IOPL is a two-bit field, separate from the Boolean flags.
    pub(crate) fn read_iopl(&mut self, body: &mut BlockBuilder<'_>) -> Result<Val<I8>, BuildError> {
        self.flags.read_iopl(body)
    }

    pub(crate) fn write_iopl(
        &mut self,
        body: &mut BlockBuilder<'_>,
        value: Val<I8>,
    ) -> Result<(), BuildError> {
        self.flags.write_iopl(body, value)
    }

    pub(crate) fn condition(
        &mut self,
        body: &mut BlockBuilder<'_>,
        condition: Condition,
    ) -> Result<Val<I1>, BuildError> {
        self.flags.condition(body, self.cpu, condition)
    }

    /// Includes completed instructions whose retirement has not yet been published.
    pub(crate) fn instruction_count(
        &self,
        body: &mut BlockBuilder<'_>,
        completed: u32,
    ) -> Result<Val<I64>, BuildError> {
        let count = cpu_load!(body, self.cpu.memory(), instruction_count)?;
        Ok(count.add(u64::from(completed)))
    }

    /// Publishes current definitions and terminates at the supplied faulting EIP.
    /// Callers define only effects that are permitted to survive this fault.
    pub(super) fn fault(
        &self,
        mut body: BlockBuilder<'_>,
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
        body: &mut BlockBuilder<'_>,
        next_eip: impl Into<Val<I32>>,
        completed: u32,
    ) -> Result<(), BuildError> {
        self.flags.publish(body, self.cpu)?;
        self.x87.publish(body)?;
        self.registers.publish(body)?;
        cpu_store!(body, self.cpu.memory(), eip, next_eip)?;
        if completed != 0 {
            let count = self.instruction_count(body, completed)?;
            cpu_store!(body, self.cpu.memory(), instruction_count, count)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
