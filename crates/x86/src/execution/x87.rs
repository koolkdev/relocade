//! x87 execution supplies restart checks and instruction/data-pointer tracking.

use wasm86_compiler::BuildError;

use crate::{state::X87Access, Segment};

use super::{memory::MemoryOperand, ExecutionBuilder};

impl<'body> ExecutionBuilder<'body, '_> {
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
