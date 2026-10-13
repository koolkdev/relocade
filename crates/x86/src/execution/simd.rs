//! Accesses to the SIMD state retained by this execution path.

use super::ExecutionBuilder;
use crate::{register::RegisterCode, simd::SimdExceptions, state::Mxcsr, Exception, StoredSimd};
use wasm86_compiler::{BuildError, Val, I32, V128};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn read_mxcsr(&mut self) -> Result<Mxcsr, BuildError> {
        self.state.read_mxcsr(&mut self.body)
    }

    /// Publish sticky status before an unmasked fault. Call before writing the
    /// instruction's XMM or EFLAGS destination, after any operand-access checks.
    pub(crate) fn record_simd_exceptions(
        &mut self,
        exceptions: &SimdExceptions,
    ) -> Result<(), BuildError> {
        let unmasked = self
            .state
            .record_simd_exceptions(&mut self.body, exceptions)?;
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
