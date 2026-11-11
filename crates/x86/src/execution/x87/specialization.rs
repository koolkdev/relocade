//! x87 admission keeps restart guards and their continuation facts together.

use relocade_compiler::{BuildError, Val, I1};

use crate::{
    execution::ExecutionBuilder,
    state::x87::X87Specialization,
    x87::{ArithmeticCandidate, ArithmeticResult},
};

/// Control facts established by current constants or successful guards.
#[derive(Default)]
pub(crate) struct X87ControlFacts {
    nearest_53: bool,
}

impl X87ControlFacts {
    /// RC alone cannot establish this fact: both PC and RC must be proven.
    pub(crate) fn nearest_53(&self) -> bool {
        self.nearest_53
    }
}

impl ExecutionBuilder<'_, '_> {
    /// Guards the requested controls and returns only facts established by them.
    /// Arithmetic also preserves the state's optional already-set PE guard.
    pub(crate) fn specialize_x87(
        &mut self,
        specialization: X87Specialization,
    ) -> Result<X87ControlFacts, BuildError> {
        Ok(self
            .if_can_fallback(|jit| {
                let observations = jit.observations.x87;
                let guard = jit
                    .x87()
                    .specialization_condition(observations, specialization)?;
                jit.guard(guard.condition)?;
                Ok(X87ControlFacts {
                    nearest_53: guard.nearest_53,
                })
            })?
            .unwrap_or_default())
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
