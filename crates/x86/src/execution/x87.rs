//! x87 execution supplies restart checks and instruction/data-pointer tracking.

use wasm86_compiler::{BuildError, I32, I64};

use crate::{
    state::{x87::X87ModeFields, X87Access},
    x87::{BinaryFormat, BinaryOperand},
    Segment,
};

use super::{memory::MemoryOperand, ExecutionBuilder};

impl MemoryOperand<'_> {
    /// Reads the opcode-defined real format after the complete span was checked.
    pub(crate) fn read_x87_binary(
        &self,
        execution: &mut ExecutionBuilder<'_, '_>,
        format: BinaryFormat,
    ) -> Result<BinaryOperand, BuildError> {
        let bits = match format {
            BinaryFormat::Binary32 => self.read::<I32>(execution, 0)?.unsigned().extend::<I64>(),
            BinaryFormat::Binary64 => self.read::<I64>(execution, 0)?,
        };
        Ok(format.decode(&bits))
    }
}

impl<'body> ExecutionBuilder<'body, '_> {
    /// Checks observed controls inside `specialize`, before instruction effects.
    /// The state owner supplies current SSA values; ordinary compiler facts
    /// remove repeated guards and specialize every use of those values.
    pub(crate) fn specialize_x87_mode(&mut self, fields: X87ModeFields) -> Result<(), BuildError> {
        if let Some(observed) = self.observed_cpu {
            let matches = self.x87().mode_matches(&observed.x87.control, fields)?;
            self.specialize_on(matches)?;
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
