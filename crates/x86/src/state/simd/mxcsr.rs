//! MXCSR controls and sticky status are distinct from the numerical calculation.

use crate::{simd::SimdExceptions, StoredSimd};
use wasm86_compiler::{Val, I1, I32};

pub(crate) struct Mxcsr(Val<I32>);

impl Mxcsr {
    pub(super) fn new(bits: Val<I32>) -> Self {
        Self(bits)
    }

    /// Architectural reads expose zero for reserved bits in a literal snapshot.
    pub(crate) fn bits(&self) -> Val<I32> {
        self.0.and(StoredSimd::MXCSR_MASK)
    }

    pub(crate) fn denormals_are_zero(&self) -> Val<I1> {
        self.0.and(1 << 6).ne(0)
    }

    /// Record newly raised conditions even when masked. Old sticky flags alone
    /// cannot fault, and every unrelated bit in the stored snapshot is retained.
    pub(super) fn record(&self, exceptions: &SimdExceptions) -> (Val<I32>, Val<I1>) {
        let operand_flags = exceptions
            .invalid
            .select(1_u32, 0)
            .or(exceptions.denormal.select(2_u32, 0));
        let result_flags = exceptions
            .overflow
            .select(8_u32, 0)
            .or(exceptions.underflow.select(16_u32, 0))
            .or(exceptions.precision.select(32_u32, 0));
        let enabled = self.0.unsigned().shr(7).xor(u32::MAX);
        // Any unmasked operand exception stops the packed instruction before
        // range or precision conditions from other lanes can be reported.
        let operand_fault = operand_flags.and(&enabled).ne(0);
        let raised = operand_flags.or(operand_fault.select(0, result_flags));
        let unmasked = raised.and(enabled).ne(0);
        (self.0.or(raised), unmasked)
    }
}
