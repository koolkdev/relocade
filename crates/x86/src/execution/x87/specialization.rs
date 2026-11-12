//! x87 admission keeps restart guards and their continuation facts together.

use relocade_compiler::{BuildError, Val, I1};

use crate::{
    execution::ExecutionBuilder,
    state::x87::X87Access,
    x87::{ArithmeticCandidate, ArithmeticResult},
    X87State,
};

type X87Check = fn(&mut X87Access<'_, '_>, &X87State) -> Result<Val<I1>, BuildError>;

impl ExecutionBuilder<'_, '_> {
    /// Combines the requested checks into one guard when fallback and a snapshot exist.
    pub(crate) fn guard_observed_x87<const N: usize>(
        &mut self,
        checks: [X87Check; N],
    ) -> Result<(), BuildError> {
        self.if_can_fallback(|jit| {
            let Some(observed) = jit.observed_cpu else {
                return Ok(());
            };
            let mut condition = Val::<I1>::from(true);
            let mut state = jit.x87();
            for check in checks {
                condition = condition.and(check(&mut state, &observed.x87)?);
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
