//! Retained SIMD state preserves raw XMM encodings and logical MXCSR bits.

mod mxcsr;
pub(crate) use mxcsr::Mxcsr;

use super::{access::cpu_location, ExecutionContext, SimdState, State};
use crate::{register::RegisterCode, simd::SimdExceptions, ssa::Location};
use relocade_compiler::{BlockBuilder, BuildError, Val, I1, I32, V128};
use std::mem::offset_of;

fn xmm_location(register: RegisterCode) -> Location<V128> {
    let base = offset_of!(ExecutionContext, cpu.simd.xmm) as u32;
    match register {
        RegisterCode::Known(code) => Location::new(base + u32::from(code & 7) * 16),
        RegisterCode::Indexed(code) => Location::indexed(base, 128, code.and(7).shl(4)),
    }
}

impl State<'_> {
    pub(crate) fn read_mxcsr(&mut self, body: &mut BlockBuilder<'_>) -> Result<Mxcsr, BuildError> {
        Ok(Mxcsr::new(self.simd.read(body, cpu_location!(simd.mxcsr))?))
    }

    pub(crate) fn record_simd_exceptions(
        &mut self,
        body: &mut BlockBuilder<'_>,
        exceptions: &SimdExceptions,
    ) -> Result<Val<I1>, BuildError> {
        let (bits, unmasked) = self.read_mxcsr(body)?.record(exceptions);
        self.simd.define(body, cpu_location!(simd.mxcsr), bits)?;
        Ok(unmasked)
    }

    pub(crate) fn write_mxcsr(
        &mut self,
        body: &mut BlockBuilder<'_>,
        value: Val<I32>,
    ) -> Result<(), BuildError> {
        self.simd.define(
            body,
            cpu_location!(simd.mxcsr),
            value.and(SimdState::MXCSR_MASK),
        )
    }

    pub(crate) fn read_xmm(
        &mut self,
        body: &mut BlockBuilder<'_>,
        register: RegisterCode,
    ) -> Result<Val<V128>, BuildError> {
        self.simd.read(body, xmm_location(register))
    }

    pub(crate) fn write_xmm(
        &mut self,
        body: &mut BlockBuilder<'_>,
        register: RegisterCode,
        value: Val<V128>,
    ) -> Result<(), BuildError> {
        self.simd.define(body, xmm_location(register), value)
    }
}
