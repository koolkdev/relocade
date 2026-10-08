//! Complete x87 operations keep capture, admission and effects in execution order.

mod operand;

pub(crate) use operand::X87Operand;

use wasm86_compiler::{BuildError, Val, I32};

use crate::{
    state::{x87::X87Specialization, Arithmetic, X87Access},
    x87::{ArithmeticResult, BinaryOperation},
    Segment,
};

use super::{memory::MemoryOperand, ExecutionBuilder};
use operand::ResolvedX87Operand;

impl<'body> ExecutionBuilder<'body, '_> {
    /// Executes binary arithmetic, including every restart boundary and effect.
    pub(crate) fn arithmetic_x87(
        &mut self,
        destination: Val<I32>,
        source: X87Operand,
        operation: BinaryOperation,
        pop: bool,
    ) -> Result<(), BuildError> {
        self.check_x87_exception()?;
        let ResolvedX87Operand { source, memory } = self.resolve_x87_operand(source)?;
        let mut arithmetic = self.x87().prepare_binary(destination, source, pop)?;
        let result = self.calculate_x87_arithmetic(&mut arithmetic, operation)?;
        self.record_x87_operand(memory.as_ref())?;
        self.x87().commit_arithmetic(arithmetic, result)
    }

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

    /// Calculates before instruction effects. The interpreter retains the full
    /// response; the JIT guards admission and consumes one numerical candidate.
    fn calculate_x87_arithmetic(
        &mut self,
        arithmetic: &mut Arithmetic,
        operation: BinaryOperation,
    ) -> Result<ArithmeticResult, BuildError> {
        if !self.can_specialize {
            return self.compute(|body| arithmetic.calculate(body, operation));
        }
        self.specialize_x87(X87Specialization::Arithmetic)?;
        self.specialize_on(arithmetic.precision_only_operands(operation))?;
        arithmetic.assume_present();

        // The control guard establishes this fact about the current SSA state.
        // Numerical selection does not need the CPU snapshot or restart policy.
        let nearest_53 = self.observed_cpu.is_some_and(|observed| {
            let control = &observed.x87.control;
            control.precision_control & 3 == 2 && control.rounding_control & 3 == 0
        });
        let candidate =
            self.compute(|body| arithmetic.rounding_candidate(body, operation, nearest_53))?;
        self.specialize_on(candidate.valid)?;
        Ok(ArithmeticResult::from_rounding(candidate.rounded))
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
