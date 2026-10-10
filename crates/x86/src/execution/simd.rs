//! Accesses to the SIMD state retained by this execution path.

use super::ExecutionBuilder;
use crate::{register::RegisterCode, Exception, StoredSimd};
use wasm86_compiler::{BuildError, Val, I32, V128};

impl ExecutionBuilder<'_, '_> {
    pub(crate) fn read_mxcsr(&mut self) -> Result<Val<I32>, BuildError> {
        self.state.read_mxcsr(&mut self.body)
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
