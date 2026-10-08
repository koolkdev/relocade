//! Complete x87 operations keep capture, admission and effects in execution order.

mod arithmetic;
mod compare;
mod conditional_move;
mod operand;

pub(crate) use compare::ComparisonTarget;
pub(crate) use operand::X87Operand;

use wasm86_compiler::BuildError;

use crate::{
    state::{x87::X87Specialization, X87Access},
    Segment,
};

use super::{memory::MemoryOperand, ExecutionBuilder};

impl<'body> ExecutionBuilder<'body, '_> {
    fn record_x87_operand(&mut self, memory: Option<&MemoryOperand<'_>>) -> Result<(), BuildError> {
        match memory {
            Some(operand) => self.record_x87_memory(operand),
            None => self.record_x87_instruction(),
        }
    }

    /// Checks observed state on a specializing path, before instruction effects.
    /// The state owner supplies current SSA values; ordinary compiler facts
    /// remove repeated guards and specialize every use of those values.
    pub(crate) fn specialize_x87(
        &mut self,
        specialization: X87Specialization,
    ) -> Result<(), BuildError> {
        if let Some(observed) = self.observed_cpu {
            let condition = self
                .x87()
                .specialization_condition(&observed.x87, specialization)?;
            self.specialize_on(condition)?;
        }
        Ok(())
    }

    pub(crate) fn x87(&mut self) -> X87Access<'_, 'body> {
        self.state.x87.access(&mut self.body)
    }

    /// Records a data instruction after its memory and specialization guards.
    /// Control instructions do not update these saved instruction fields.
    pub(crate) fn record_x87_instruction(&mut self) -> Result<(), BuildError> {
        let selector = self
            .state
            .read_segment_selector(&mut self.body, &Segment::Cs.into())?;
        let opcode = self
            .x87_opcode
            .as_ref()
            .expect("x87 forms retain their opcode");
        self.state
            .x87
            .access(&mut self.body)
            .record_instruction(&self.eip, selector, opcode)
    }

    pub(crate) fn record_x87_memory(
        &mut self,
        operand: &MemoryOperand<'_>,
    ) -> Result<(), BuildError> {
        self.record_x87_instruction()?;
        let selector = self
            .state
            .read_segment_selector(&mut self.body, operand.segment())?;
        self.x87().record_data(operand.offset(), selector)
    }

    /// Delivers a pending exception at this instruction's restart boundary.
    /// Called by FWAIT and x87 instructions that check exceptions on entry.
    pub(crate) fn check_x87_exception(&mut self) -> Result<(), BuildError> {
        self.state
            .check_x87(&mut self.body, &self.eip, self.completed)
    }
}
