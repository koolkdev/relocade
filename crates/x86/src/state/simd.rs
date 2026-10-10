//! Retained SIMD state preserves raw XMM encodings and logical MXCSR bits.

use super::{access::cpu_location, CpuState, State, StoredSimd};
use crate::{register::RegisterCode, ssa::Location};
use std::mem::offset_of;
use wasm86_compiler::{BlockBuilder, BuildError, Val, I32, V128};

fn xmm_location(register: RegisterCode) -> Location<V128> {
    let base = offset_of!(CpuState, simd.xmm) as u32;
    match register {
        RegisterCode::Known(code) => Location::new(base + u32::from(code & 7) * 16),
        RegisterCode::Indexed(code) => Location::indexed(base, 128, code.and(7).shl(4)),
    }
}

impl State<'_> {
    pub(crate) fn read_mxcsr(
        &mut self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I32>, BuildError> {
        // Literal snapshots retain reserved bits; architectural reads expose zero.
        Ok(self
            .simd
            .read(body, cpu_location!(simd.mxcsr))?
            .and(StoredSimd::MXCSR_MASK))
    }

    pub(crate) fn write_mxcsr(
        &mut self,
        body: &mut BlockBuilder<'_>,
        value: Val<I32>,
    ) -> Result<(), BuildError> {
        self.simd.define(
            body,
            cpu_location!(simd.mxcsr),
            value.and(StoredSimd::MXCSR_MASK),
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
