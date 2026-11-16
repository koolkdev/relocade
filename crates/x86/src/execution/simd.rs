//! Accesses to the SIMD state retained by this execution path.

use super::ExecutionBuilder;
use crate::{
    register::RegisterSelector,
    simd::{FloatResult, PackedFloatCandidate, SimdExceptions},
    state::Mxcsr,
    Exception, SimdState,
};
use relocade_compiler::{BuildError, Val, I32, V128};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn read_mxcsr(&mut self) -> Result<Mxcsr, BuildError> {
        self.state.read_mxcsr(&mut self.body)
    }

    /// Numerical operations can use observed controls only after a restart
    /// guard establishes them. Architectural state reads remain unguarded.
    pub(crate) fn simd_controls(&mut self) -> Result<Mxcsr, BuildError> {
        if let Some(observed) = self.observed_cpu {
            self.specialize_state(&[Mxcsr::control_pattern(observed.simd.mxcsr)])?;
        }
        self.read_mxcsr()
    }

    /// Precision-producing operations can skip exactness work when masked PE is
    /// already set. Read controls after establishing that optional fact.
    pub(crate) fn simd_precision_controls(&mut self) -> Result<Mxcsr, BuildError> {
        if let Some(observed) = self.observed_cpu {
            let mut pattern = vec![Mxcsr::control_pattern(observed.simd.mxcsr)];
            pattern.extend(Mxcsr::precision_pattern(observed.simd.mxcsr));
            self.specialize_state(&pattern)?;
        }
        self.read_mxcsr()
    }

    /// Generic arithmetic blocks prefer nearest rounding. Observed blocks use
    /// the recorded rounding mode; interpreters retain all modes.
    pub(crate) fn simd_arithmetic_controls(&mut self) -> Result<Mxcsr, BuildError> {
        if self.observed_cpu.is_none() {
            self.specialize_state(&[Mxcsr::nearest_rounding_pattern()])?;
            self.read_mxcsr()
        } else {
            self.simd_precision_controls()
        }
    }

    /// Requires interpreter fallback. Guard the candidate's numerical domain
    /// before publishing its result or precision effects.
    pub(crate) fn admit_simd_candidate(
        &mut self,
        candidate: PackedFloatCandidate,
    ) -> Result<FloatResult<V128>, BuildError> {
        self.guard(candidate.valid)?;
        Ok(candidate.result)
    }

    /// Publish sticky status before an unmasked fault. Call before writing the
    /// instruction's destination, after any operand-access checks.
    pub(crate) fn record_simd_exceptions(
        &mut self,
        exceptions: &SimdExceptions,
    ) -> Result<(), BuildError> {
        // Operand and range exceptions are uncommon. Precision loss is common
        // in numerical operations, so keep its status and fault response compiled.
        let mut exceptions = exceptions.clone();
        self.guard_and_clear([
            &mut exceptions.invalid,
            &mut exceptions.denormal,
            &mut exceptions.zero_divide,
            &mut exceptions.overflow,
            &mut exceptions.underflow,
        ])?;
        let unmasked = self
            .state
            .record_simd_exceptions(&mut self.body, &exceptions)?;
        self.fault_if(unmasked, Exception::SimdFloatingPoint)
    }

    pub(crate) fn load_mxcsr(&mut self, value: Val<I32>) -> Result<(), BuildError> {
        self.fault_if(
            value.and(!SimdState::MXCSR_MASK).ne(0),
            Exception::GeneralProtection { error_code: 0 },
        )?;
        self.state.write_mxcsr(&mut self.body, value)
    }

    pub(crate) fn read_xmm(&mut self, register: RegisterSelector) -> Result<Val<V128>, BuildError> {
        self.state.read_xmm(&mut self.body, register)
    }

    pub(crate) fn write_xmm(
        &mut self,
        register: RegisterSelector,
        value: Val<V128>,
    ) -> Result<(), BuildError> {
        self.state.write_xmm(&mut self.body, register, value)
    }
}

#[cfg(test)]
mod tests;
