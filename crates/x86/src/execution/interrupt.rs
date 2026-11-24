//! Real-mode entry shared by software INT, faults and acknowledged external requests.

use super::{stack::StackAccess, CodeTarget, ExecutionBuilder};
use crate::{
    flags::{image, Flag},
    memory::{Access, Intent},
    real_mode::{FarPointer, InterruptFrame},
    ExceptionCase, ExceptionValues, ExceptionVector, ExecutionExitValues, Segment,
};
use relocade_compiler::{BuildError, Val, I1, I16, I32, I8};

/// The stack and IVT share one availability decision; a slow transfer can
/// change routing for later transfers in either span.
struct InterruptEntry {
    stack: StackAccess,
    vector: Access,
}

impl InterruptEntry {
    fn check(
        execution: &mut ExecutionBuilder<'_, '_>,
        vector: &Val<I8>,
    ) -> Result<Self, BuildError> {
        let bytes = InterruptFrame::BYTE_LEN as u32;
        let mut stack = execution
            .stack_pointer()?
            .advance(0u32.wrapping_sub(bytes))
            .check_access::<I16>(execution, 3, Intent::Write)?;
        let address = vector
            .unsigned()
            .extend::<I32>()
            .mul(FarPointer::BYTE_LEN as u32);
        let mut vector = execution.accesses.memory().check_access(
            &mut execution.body,
            &address,
            FarPointer::BYTE_LEN as u32,
            Intent::Read,
            None,
            None,
        )?;
        execution.guard_accesses(true, [&mut stack.access, &mut vector])?;
        Ok(Self { stack, vector })
    }

    fn enter(
        self,
        execution: &mut ExecutionBuilder<'_, '_>,
        return_ip: Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        // A slow entry still checks the complete frame before its first write.
        let bytes = InterruptFrame::BYTE_LEN as u32;
        let pointer = execution.stack_pointer()?.advance(0u32.wrapping_sub(bytes));
        execution.translate(&Segment::Ss.into(), &pointer.offset(), bytes, Intent::Write)?;
        let flags = image::read_stack_image::<I16>(execution)?;
        let cs = execution.read_segment_selector(Segment::Cs)?;
        self.stack
            .field::<I16>(execution, InterruptFrame::FLAGS_OFFSET / 2)?
            .write(execution, &flags)?;
        execution.write_flag(Flag::IF, false)?;
        execution.write_flag(Flag::TF, false)?;
        execution.write_flag(Flag::AC, false)?;
        self.stack
            .field::<I16>(execution, InterruptFrame::CS_OFFSET / 2)?
            .write(execution, &cs)?;
        self.stack
            .field::<I16>(execution, InterruptFrame::IP_OFFSET / 2)?
            .write(execution, &return_ip.truncate::<I16>())?;

        // Read after the pushes: the stack may alias the conventional real IVT.
        let memory = execution.accesses.memory();
        let selector = memory.read::<I16>(
            &mut execution.body,
            &self.vector,
            FarPointer::SEGMENT_OFFSET,
        )?;
        let offset = memory.read::<I16>(&mut execution.body, &self.vector, 0)?;
        let target = CodeTarget::resolve(execution, offset, &selector)?;
        target.check_limit(execution)?;
        pointer.commit(execution)?;
        target.commit(execution)
    }
}

impl ExecutionBuilder<'_, '_> {
    /// Publishes completed work without retiring the faulting instruction.
    pub(crate) fn fault(
        &mut self,
        exception: impl Into<ExceptionValues>,
    ) -> Result<(), BuildError> {
        self.fault_if(true, exception)
    }

    /// Ends a faulting path with the current state. Define only effects permitted
    /// to survive that fault; the instruction's EIP and retirement stay at entry.
    pub(crate) fn fault_if(
        &mut self,
        condition: impl Into<Val<I1>>,
        exception: impl Into<ExceptionValues>,
    ) -> Result<(), BuildError> {
        let protection_enabled = self.state.protection_enabled(&mut self.body)?;
        self.body.if_(condition, |mut body| {
            self.state.publish(&mut body, &self.eip, self.completed)?;
            let exception = exception.into();

            // The IVT entry helper has no handler of its own. Faults during
            // entry return to the host to avoid recursive delivery.
            let Some(interrupt) = self.interrupt else {
                return crate::state::exit::exception(body, exception);
            };
            body.if_(protection_enabled, |body| {
                crate::state::exit::exception(body, exception.clone())
            })?;
            exception.clone().match_(&mut body, |body, case| {
                let vector = match case {
                    ExceptionCase::DivideError => ExceptionVector::DivideError,
                    ExceptionCase::BoundRangeExceeded => ExceptionVector::BoundRangeExceeded,
                    ExceptionCase::InvalidOpcode => ExceptionVector::InvalidOpcode,
                    _ => return crate::state::exit::exception(body, exception.clone()),
                };
                interrupt.tail_call(body, vector as u32)
            })?;
            body.trap()
        })
    }

    /// Software INT supplies its fallthrough; external entry saves the current IP.
    pub(crate) fn enter_interrupt(
        &mut self,
        vector: Val<I8>,
        return_ip: Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        let entry = InterruptEntry::check(self, &vector)?;
        entry.enter(self, return_ip)
    }

    /// Publishes the ISR position without executing it or retiring an instruction.
    pub(crate) fn deliver_interrupt(mut self, vector: Val<I8>) -> Result<(), BuildError> {
        let target = self.enter_interrupt(vector, self.eip.clone())?;
        self.state.publish(&mut self.body, target, 0)?;
        self.body.return_(ExecutionExitValues::interrupt_entered())
    }

    /// Completes the real-mode instruction and inhibits interrupts for its successor.
    /// Call only after all of the instruction's effects and fault checks.
    pub(crate) fn interrupt_inhibited_if(
        &mut self,
        condition: impl Into<Val<I1>>,
        next_ip: &Val<I32>,
    ) -> Result<(), BuildError> {
        let real = self.real_mode()?;
        self.body.if_(real.and(condition), |mut body| {
            self.state.publish(&mut body, next_ip, self.completed + 1)?;
            body.return_(ExecutionExitValues::interrupt_inhibited())
        })
    }

    /// A flag restore opened an IRQ window without an inhibited successor.
    /// This is called after committing every instruction effect and fault check.
    pub(crate) fn interrupts_enabled_if(
        &mut self,
        condition: Val<I1>,
        next_ip: &Val<I32>,
    ) -> Result<(), BuildError> {
        let real = self.real_mode()?;
        self.body.if_(real.and(condition), |mut body| {
            self.state.publish(&mut body, next_ip, self.completed + 1)?;
            self.state.checkpoint(&mut body)?;
            body.return_(ExecutionExitValues::interrupts_enabled())
        })
    }
}
