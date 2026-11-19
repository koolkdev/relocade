//! x87 restart guards keep fallback and continuation facts together.

use relocade_compiler::{BuildError, Val, I1};

use crate::{
    execution::ExecutionBuilder,
    state::x87::X87Specialization,
    x87::{ArithmeticCandidate, ArithmeticResult},
};

impl ExecutionBuilder<'_, '_> {
    /// Guards the observed x87 preferences before retaining their facts.
    pub(crate) fn specialize_x87(
        &mut self,
        specialization: X87Specialization,
    ) -> Result<(), BuildError> {
        if let Some(observed) = self.observed_cpu {
            self.guard_specialization(&specialization.patterns(&observed.x87))?;
        }
        Ok(())
    }

    /// Returns the push-slot overflow condition. A successful admission
    /// proves the slot is empty on the continuing path.
    pub(crate) fn guard_x87_push(
        &mut self,
        source_valid: impl Into<Val<I1>>,
    ) -> Result<Val<I1>, BuildError> {
        let full = self.x87().push_full()?;
        let guarded =
            self.if_can_fallback(|jit| jit.guard(source_valid.into().and(full.eq(false))))?;
        Ok(if guarded.is_some() {
            false.into()
        } else {
            full
        })
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
