//! MXCSR controls and sticky status are distinct from the numerical calculation.

use crate::{
    floating::RoundingMode,
    simd::SimdExceptions,
    state::{access::cpu_location, StatePattern},
    SimdState,
};
use relocade_compiler::{Val, I1, I32, I8};

pub(crate) struct Mxcsr(Val<I32>);

impl Mxcsr {
    pub(super) fn new(bits: Val<I32>) -> Self {
        Self(bits)
    }

    /// Architectural reads expose zero for reserved bits in a literal snapshot.
    pub(crate) fn bits(&self) -> Val<I32> {
        self.0.and(SimdState::MXCSR_MASK)
    }

    pub(crate) fn control_pattern(observed: u32) -> StatePattern {
        const CONTROLS: u32 = 0xffc0;
        StatePattern::bits(
            cpu_location!(simd.mxcsr),
            u64::from(CONTROLS),
            u64::from(observed),
        )
    }

    pub(crate) fn precision_pattern(observed: u32) -> Option<StatePattern> {
        const PRECISION: u32 = 1 << 5;
        const PRECISION_MASK: u32 = 1 << 12;
        let masked_precision = PRECISION | PRECISION_MASK;
        (observed & masked_precision == masked_precision).then(|| {
            StatePattern::bits(
                cpu_location!(simd.mxcsr),
                u64::from(masked_precision),
                u64::from(masked_precision),
            )
        })
    }

    pub(crate) fn nearest_rounding_pattern() -> StatePattern {
        const ROUNDING_CONTROL: u64 = 0b11 << 13;
        StatePattern::bits(cpu_location!(simd.mxcsr), ROUNDING_CONTROL, 0)
    }

    pub(crate) fn denormals_are_zero(&self) -> Val<I1> {
        self.0.and(1 << 6).ne(0)
    }

    pub(crate) fn rounding(&self) -> RoundingMode {
        RoundingMode::new(self.0.unsigned().shr(13).truncate::<I8>())
    }

    pub(crate) fn overflow_masked(&self) -> Val<I1> {
        self.0.and(1 << 10).ne(0)
    }

    pub(crate) fn underflow_masked(&self) -> Val<I1> {
        self.0.and(1 << 11).ne(0)
    }

    pub(crate) fn flush_to_zero(&self) -> Val<I1> {
        self.0.and(1 << 15).ne(0)
    }

    /// Record newly raised conditions even when masked. Old sticky flags alone
    /// cannot fault, and every unrelated bit in the stored snapshot is retained.
    pub(super) fn record(&self, exceptions: &SimdExceptions) -> (Val<I32>, Val<I1>) {
        let operand_flags = exceptions
            .invalid
            .select(1_u32, 0)
            .or(exceptions.denormal.select(2_u32, 0))
            .or(exceptions.zero_divide.select(4_u32, 0));
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
