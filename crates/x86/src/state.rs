mod access;
mod context;
mod cpu;
pub(super) mod exit;
mod flags;
mod layout;
#[cfg(test)]
mod observation;
mod segments;
mod simd;
mod system;
pub(crate) mod x87;

pub use context::ExecutionContext;
pub(super) use cpu::Cpu;
pub use exit::{
    ExecutionExit, ExecutionExitCase, ExecutionExitValues, HostExit, HostExitCase, HostExitValues,
};
pub use layout::{
    CpuMode, CpuState, DescriptorTableRegister, FlagBytes, FlagsState, Registers, SegmentRegister,
    Segments, SimdState, StatusSourceState, SystemSegmentRegister, X87Control, X87Register,
    X87State, X87Status,
};
#[cfg(test)]
pub(crate) use observation::compile_flag_observer;
pub(crate) use segments::SegmentReads;
pub(crate) use simd::Mxcsr;
pub(crate) use system::DescriptorTableValues;
pub(crate) use x87::X87Access;

use access::{cpu_load, cpu_store, register_location};
use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32, I64, I8};

use crate::{
    exception::ExceptionValues,
    flags::{Condition, Flag, FlagChange},
    register::{Register, RegisterType},
    segment::{SegmentSelection, SegmentValues},
    ssa::StateFields,
};

#[derive(Clone)]
pub(super) struct State<'cpu> {
    cpu: &'cpu Cpu,
    instruction_budgets: bool,
    registers: StateFields,
    simd: StateFields,
    flags: flags::FlagTracker,
    pub(super) x87: x87::X87Tracker,
}

impl<'cpu> State<'cpu> {
    pub(super) fn read_cpu_features(
        &self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I32>, BuildError> {
        body.load_at::<I32>(
            self.cpu.memory(),
            0,
            std::mem::offset_of!(ExecutionContext, cpu_model) as u32,
        )
    }

    pub(super) fn new(cpu: &'cpu Cpu) -> Self {
        Self {
            cpu,
            instruction_budgets: true,
            registers: StateFields::new(cpu.memory()),
            simd: StateFields::new(cpu.memory()),
            flags: flags::FlagTracker::new(cpu.memory()),
            x87: x87::X87Tracker::new(cpu.memory()),
        }
    }

    pub(super) fn with_instruction_budgets(mut self, enabled: bool) -> Self {
        self.instruction_budgets = enabled;
        self
    }

    /// Installs a segment after all instruction checks. Only publication and
    /// dispatch may follow; segment reads belong to the entry's old state.
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
        exception: ExceptionValues,
    ) -> Result<(), BuildError> {
        self.publish(&mut body, restart_eip, completed)?;
        exit::exception(body, exception)
    }

    /// Checks execution controls after publishing a completed boundary.
    /// Interrupt requests take precedence over an exhausted instruction budget.
    pub(crate) fn checkpoint(&self, body: &mut BlockBuilder<'_>) -> Result<(), BuildError> {
        let interrupt_requested = body
            .atomic::<I8>(
                self.cpu.memory(),
                0,
                std::mem::offset_of!(ExecutionContext, interrupt_requested) as u32,
            )?
            .load()?;
        body.if_(interrupt_requested.ne(0), |body| {
            body.return_(ExecutionExitValues::interrupted())
        })?;
        if self.instruction_budgets {
            let target = body.load_at::<I64>(
                self.cpu.memory(),
                0,
                std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
            )?;
            body.if_(target.ne(0), |mut body| {
                let count = self.instruction_count(&mut body, 0)?;
                body.if_(count.sub(target).signed().ge(0), |body| {
                    body.return_(ExecutionExitValues::budget_exhausted())
                })
            })?;
        }
        Ok(())
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
        self.simd.publish(body)?;
        self.registers.publish(body)?;
        cpu_store!(body, self.cpu.memory(), eip, next_eip)?;
        if completed != 0 {
            let count = self.instruction_count(body, completed)?;
            cpu_store!(body, self.cpu.memory(), instruction_count, &count)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
