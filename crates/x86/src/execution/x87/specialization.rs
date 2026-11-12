//! x87 admission keeps restart guards and their continuation facts together.

use relocade_compiler::{BuildError, Val, I1};

use crate::{
    execution::ExecutionBuilder,
    state::x87::X87Specialization,
    x87::{ArithmeticCandidate, ArithmeticResult},
};

impl ExecutionBuilder<'_, '_> {
    /// Guards observed state so the compiler can simplify subsequent calculations.
    pub(crate) fn guard_x87_controls(
        &mut self,
        specialization: X87Specialization,
    ) -> Result<(), BuildError> {
        self.if_can_fallback(|jit| {
            let Some(observed) = jit.observed_cpu else {
                return Ok(());
            };
            // All callers use rounding control (RC).
            let mut condition = jit
                .x87()
                .observed_rounding_condition(observed.x87.control.rounding_control)?;
            if matches!(specialization, X87Specialization::Arithmetic) {
                // FADD/FSUB/FMUL/FDIV and FSQRT also use precision control (PC).
                let precision = jit
                    .x87()
                    .observed_precision_condition(observed.x87.control.precision_control)?;
                condition = condition.and(precision);

                // PM=1 and PE=1 let repeated precision-exception updates fold away.
                let precision_masked = observed.x87.control.precision_mask & 1 != 0;
                let precision_raised = observed.x87.status.precision & 1 != 0;
                if precision_masked && precision_raised {
                    condition = condition.and(jit.x87().observed_masked_precision_condition()?);
                }
            }
            jit.guard(condition)
        })?;
        Ok(())
    }

    /// Keeps the source policy with the load and owns the shared push-slot check.
    pub(crate) fn guard_x87_push(
        &mut self,
        source_valid: impl Into<Val<I1>>,
    ) -> Result<(), BuildError> {
        self.if_can_fallback(|jit| {
            let available = jit.x87().push_available()?;
            jit.guard(source_valid.into().and(available))
        })?;
        Ok(())
    }

    /// Admits the instruction's operand domain and excludes stack faults.
    /// A successful guard lets the continuing path discard stack-fault evidence.
    /// Returns whether guards were built; paths without fallback retain the evidence.
    pub(crate) fn guard_x87_operands(
        &mut self,
        stack_fault: &mut Val<I1>,
        operands_valid: impl Into<Val<I1>>,
    ) -> Result<bool, BuildError> {
        Ok(self
            .if_can_fallback(|jit| {
                jit.guard(stack_fault.eq(false).and(operands_valid))?;
                *stack_fault = false.into();
                Ok(())
            })?
            .is_some())
    }

    /// Requires interpreter fallback and guarded operands. A candidate
    /// proves only its numerical range; after guarding it, only rounding remains.
    pub(crate) fn admit_x87_candidate(
        &mut self,
        candidate: ArithmeticCandidate,
    ) -> Result<ArithmeticResult, BuildError> {
        self.guard(&candidate.valid)?;
        Ok(ArithmeticResult::from_rounding(candidate.rounded))
    }
}
