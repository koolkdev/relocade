//! Conditional moves keep value selection and exception-suppressed writes distinct.

use wasm86_compiler::{BuildError, Val, I32};

use crate::{
    flags::Condition,
    state::x87::{C1Update, StackValue},
};

use super::ExecutionBuilder;

impl ExecutionBuilder<'_, '_> {
    /// Both outcomes stay compiled; only empty operands restart in the interpreter.
    pub(crate) fn conditional_move_x87(
        &mut self,
        source: Val<I32>,
        condition: Condition,
    ) -> Result<(), BuildError> {
        self.check_x87_exception()?;
        let condition = self.condition(condition)?;
        // Intel AP-526 section 4.2.2.1.2 requires both stack operands even
        // when the condition is false, and leaves C1 unchanged without #IS.
        // Read the dynamic source first: runtime decoding may rebase register
        // tracking when it first sees that index.
        let source = self.x87().read_stack(source)?;
        let previous = self.x87().read_stack(0)?;
        let value = source.value.select(&condition, &previous.value);
        let tag = condition.select(source.value.tag(), &previous.tag);
        let mut stack_fault = previous.is_empty().or(source.is_empty());
        self.specialize(|execution| {
            execution.specialize_on(stack_fault.eq(false))?;
            stack_fault = false.into();
            Ok(())
        })?;
        self.record_x87_instruction()?;

        let mut x87 = self.x87();
        let enabled = x87.stack_underflow(&stack_fault, C1Update::ClearOnFault)?;
        let value = value.or_indefinite(&stack_fault);
        let tag = stack_fault.select(2_u32, tag);
        // The ordinary condition selects a value, not a discardable exception
        // write guard: a later waiting instruction must retain an untaken move.
        x87.write_stack(0, &StackValue { value, tag }, &enabled)
    }
}
