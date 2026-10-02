//! Numerical classes reuse established predicates before inspecting raw encodings.

use wasm86_compiler::{Val, I1};

use super::ExtendedValue;

impl ExtendedValue {
    pub(crate) fn normal(&self) -> Val<I1> {
        self.normal.clone().unwrap_or_else(|| self.bits().normal())
    }

    /// Retains a guarantee established by the arithmetic operation's guards.
    pub(in crate::x87) fn assume_normal(mut self) -> Self {
        self.normal = Some(true.into());
        self
    }

    fn non_normal(&self, condition: Val<I1>) -> Val<I1> {
        match &self.normal {
            Some(normal) => normal.eq(false).and(condition),
            None => condition,
        }
    }

    pub(in crate::x87) fn zero(&self) -> Val<I1> {
        let bits = self.bits();
        self.non_normal(bits.exponent_field().eq(0).and(bits.significand.eq(0_u64)))
    }

    pub(in crate::x87) fn special_exponent(&self) -> Val<I1> {
        self.non_normal(self.bits().exponent_field().eq(0x7fff))
    }

    pub(in crate::x87) fn nan(&self) -> Val<I1> {
        self.special_exponent().and(
            self.bits()
                .significand
                .and(0x7fff_ffff_ffff_ffff_u64)
                .ne(0_u64),
        )
    }

    pub(in crate::x87) fn signaling_nan(&self) -> Val<I1> {
        self.nan()
            .and(self.bits().significand.and(1_u64 << 62).eq(0_u64))
    }

    pub(in crate::x87) fn infinity(&self) -> Val<I1> {
        self.special_exponent().and(
            self.bits()
                .significand
                .and(0x7fff_ffff_ffff_ffff_u64)
                .eq(0_u64),
        )
    }

    pub(in crate::x87) fn denormal(&self) -> Val<I1> {
        let bits = self.bits();
        self.non_normal(bits.exponent_field().eq(0).and(bits.significand.ne(0_u64)))
    }

    pub(in crate::x87) fn unsupported(&self) -> Val<I1> {
        let bits = self.bits();
        self.non_normal(
            bits.exponent_field()
                .ne(0)
                .and(bits.significand.and(1_u64 << 63).eq(0_u64)),
        )
    }
}
