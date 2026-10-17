//! x87 execution access owns restart checks, observation guards and instruction metadata.

mod operand;
mod specialization;

pub(crate) use operand::{X87MemoryFormat, X87Operand, X87Operands};

use wasm86_compiler::BuildError;

use crate::{state::X87Access, Segment};

use super::{memory::MemoryOperand, ExecutionBuilder};

impl<'body> ExecutionBuilder<'body, '_> {
    pub(crate) fn record_x87_operand(
        &mut self,
        memory: Option<&MemoryOperand<'_>>,
    ) -> Result<(), BuildError> {
        match memory {
            Some(operand) => self.record_x87_memory(operand),
            None => self.record_x87_instruction(),
        }
    }

    pub(crate) fn x87(&mut self) -> X87Access<'_, 'body> {
        self.state.x87.access(&mut self.body)
    }

    /// Records an x87 instruction after its memory checks and fallback guards.
    /// Environment controls leave these fields unchanged, except for FNOP.
    pub(crate) fn record_x87_instruction(&mut self) -> Result<(), BuildError> {
        let selector = self
            .segments
            .read_selector(&mut self.body, &Segment::Cs.into())?;
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
            .segments
            .read_selector(&mut self.body, operand.segment())?;
        self.x87().record_data(operand.offset(), selector)
    }

    /// Delivers a pending exception at this instruction's restart boundary.
    /// Called by FWAIT and x87 instructions that check exceptions on entry.
    pub(crate) fn check_x87_exception(&mut self) -> Result<(), BuildError> {
        let pending = self.state.x87.exception_pending(&mut self.body)?;
        self.fault_if(pending, crate::exception::Exception::FloatingPoint)?;
        self.state.x87.assume_no_pending_exception(&mut self.body)
    }
}
