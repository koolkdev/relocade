//! x87 admission keeps restart guards and their continuation facts together.

use relocade_compiler::{BuildError, Val, I1};

use crate::{
    execution::ExecutionBuilder,
    state::x87::X87Specialization,
    x87::{ArithmeticCandidate, ArithmeticResult},
};

impl ExecutionBuilder<'_, '_> {
    /// Checks the observed fields used to simplify this instruction's calculation.
    /// The caller selects the fields because not every x87 operation consumes PC:
    ///
    /// - `Rounding` compares rounding control (RC & 3) with the snapshot. Memory
    ///   stores that round, FRNDINT and transcendental operations use this; their
    ///   precision follows the operation or destination format rather than PC.
    /// - `Arithmetic` also compares precision control (PC & 3), for the
    ///   FADD/FSUB/FMUL/FDIV families and FSQRT. If the snapshot has both precision
    ///   masked (PM = 1) and the sticky precision flag set (PE = 1), it also
    ///   checks PM and PE.
    ///   Another inexact result then needs no PE update or precision-exception
    ///   delivery; numerical rounding and C1 still follow the current controls.
    ///
    /// Only unknown values need snapshot checks. Values already determined by
    /// guest instructions supersede the snapshot, even when they differ. A failed
    /// check restarts the current instruction in the interpreter. Without an
    /// observation or fallback capability, this adds no guards.
    /// Operand and result guards separately decide which cases stay compiled;
    /// these observation checks do not require a particular RC or PC encoding.
    pub(crate) fn guard_x87_controls(
        &mut self,
        specialization: X87Specialization,
    ) -> Result<(), BuildError> {
        self.if_can_fallback(|jit| {
            let Some(observed) = jit.observed_cpu else {
                return Ok(());
            };
            let arithmetic = matches!(specialization, X87Specialization::Arithmetic);
            let precision = arithmetic.then_some(observed.x87.control.precision_control);
            let masked_precision_is_set = arithmetic
                && observed.x87.control.precision_mask & observed.x87.status.precision & 1 != 0;
            let condition = jit.x87().specialization_condition(
                Some(observed.x87.control.rounding_control),
                precision,
                masked_precision_is_set,
            )?;
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
