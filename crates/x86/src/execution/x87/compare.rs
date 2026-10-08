//! Comparisons read, guard and update condition codes and stack state together.

use wasm86_compiler::BuildError;

use crate::{state::x87::Exception, x87::ComparisonKind};

use super::{operand::X87Operands, ExecutionBuilder, X87Operand};

impl ExecutionBuilder<'_, '_> {
    /// Ordinary comparisons, including FUCOM quiet NaNs, stay in the block.
    /// Operand exceptions restart before metadata, condition codes or pops.
    pub(crate) fn compare_x87(
        &mut self,
        source: X87Operand,
        kind: ComparisonKind,
        pops: u32,
    ) -> Result<(), BuildError> {
        self.check_x87_exception()?;
        let X87Operands {
            values,
            mut stack_fault,
            memory,
        } = self.read_x87_operands(0.into(), source)?;
        let mut result = values.compare(kind);
        self.specialize(|execution| {
            execution.specialize_on(
                stack_fault
                    .or(&result.invalid)
                    .or(&result.denormal)
                    .eq(false),
            )?;
            stack_fault = false.into();
            result.invalid = false.into();
            result.denormal = false.into();
            Ok(())
        })?;
        self.record_x87_operand(memory.as_ref())?;

        // Empty stack entries override numerical responses, including a
        // denormal memory source that expanded to a normal extended value.
        result.invalid = stack_fault.or(result.invalid);
        result.unordered = stack_fault.or(result.unordered);
        result.denormal = stack_fault.eq(false).and(result.denormal);
        let state = &mut self.state.x87;
        let body = &mut self.body;
        let unmasked_invalid = state.status.record_exception(
            body,
            Exception::Invalid,
            &result.invalid,
            &mut state.control,
        )?;
        let unmasked_denormal = state.status.record_exception(
            body,
            Exception::Denormal,
            &result.denormal,
            &mut state.control,
        )?;
        state.status.record_stack_fault(body, &stack_fault)?;
        let suppressed = unmasked_invalid.or(unmasked_denormal);
        state
            .status
            .record_pending_exception(body, suppressed.clone())?;
        state.status.set_c1(body, false)?;
        // Unmasked invalid preserves the condition codes. Retain them for an
        // unmasked denormal too, as the pre-operation policy; Intel specifies
        // unchanged TOP and operands for #D but not the condition-code response.
        let enabled = suppressed.eq(false);
        state.status.set_comparison(body, &result, &enabled)?;
        state.access(body).pop(pops, &enabled)
    }
}
