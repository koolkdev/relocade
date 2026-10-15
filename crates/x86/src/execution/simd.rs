//! Accesses to the SIMD state retained by this execution path.

use super::ExecutionBuilder;
use crate::{register::RegisterCode, simd::SimdExceptions, state::Mxcsr, Exception, StoredSimd};
use wasm86_compiler::{BuildError, Val, I32, V128};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn read_mxcsr(&mut self) -> Result<Mxcsr, BuildError> {
        self.state.read_mxcsr(&mut self.body)
    }

    /// Guard observed numerical settings and any usable sticky precision fact
    /// before operand reads. Architectural state reads remain unguarded.
    pub(crate) fn simd_controls(&mut self) -> Result<Mxcsr, BuildError> {
        let controls = self.read_mxcsr()?;
        self.specialize(|jit| {
            if let Some(observed) = jit.observed_cpu {
                jit.specialize_on(controls.specialization_condition(observed.simd.mxcsr))?;
            }
            Ok(())
        })?;
        Ok(controls)
    }

    /// Native SIMD arithmetic needs nearest rounding. Keep an observed directed
    /// mode compiled through the general calculation; generic blocks admit the
    /// usual nearest mode and restart if the live setting differs.
    pub(crate) fn specialize_simd_nearest(&mut self, controls: &Mxcsr) -> Result<bool, BuildError> {
        Ok(self
            .specialize(|jit| {
                if jit
                    .observed_cpu
                    .is_some_and(|cpu| cpu.simd.mxcsr & 0x6000 != 0)
                {
                    return Ok(false);
                }
                jit.specialize_on(controls.bits().and(0x6000).eq(0))?;
                Ok(true)
            })?
            .unwrap_or(false))
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
        self.specialize_without([
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
            value.and(!StoredSimd::MXCSR_MASK).ne(0),
            Exception::GeneralProtection {
                error_code: 0.into(),
            },
        )?;
        self.state.write_mxcsr(&mut self.body, value)
    }

    pub(crate) fn read_xmm(&mut self, register: RegisterCode) -> Result<Val<V128>, BuildError> {
        self.state.read_xmm(&mut self.body, register)
    }

    pub(crate) fn write_xmm(
        &mut self,
        register: RegisterCode,
        value: Val<V128>,
    ) -> Result<(), BuildError> {
        self.state.write_xmm(&mut self.body, register, value)
    }
}

#[cfg(test)]
mod tests;
