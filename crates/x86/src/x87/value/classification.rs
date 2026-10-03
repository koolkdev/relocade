//! Numerical classes reuse established predicates before inspecting raw encodings.

use wasm86_compiler::{Val, I1, I8};

use super::ExtendedValue;

/// Only these exact classes are currently established by value construction.
/// Other encodings keep no class hint and use the predicates below.
enum Class {
    Normal = 0,
    Zero = 1,
    QuietNaN = 2,
}

/// An exact class established by construction or a successful guard.
#[derive(Clone)]
pub(in crate::x87) struct Classification(pub(super) Val<I8>);

impl Classification {
    pub(in crate::x87) fn normal() -> Self {
        Self((Class::Normal as u32).into())
    }

    pub(in crate::x87) fn zero() -> Self {
        Self((Class::Zero as u32).into())
    }

    pub(super) fn quiet_nan() -> Self {
        Self((Class::QuietNaN as u32).into())
    }

    pub(in crate::x87) fn select(&self, condition: &Val<I1>, otherwise: &Self) -> Self {
        Self(condition.select(&self.0, &otherwise.0))
    }
}

impl ExtendedValue {
    pub(crate) fn normal(&self) -> Val<I1> {
        self.class
            .as_ref()
            .map(|class| class.0.eq(Class::Normal as u32))
            .unwrap_or_else(|| self.bits().normal())
    }

    /// The caller must establish this exact class wherever the value is used.
    pub(in crate::x87) fn assume_class(mut self, class: Classification) -> Self {
        self.class = Some(class);
        self
    }

    pub(in crate::x87) fn zero(&self) -> Val<I1> {
        if let Some(class) = &self.class {
            return class.0.eq(Class::Zero as u32);
        }
        let bits = self.bits();
        bits.exponent_field().eq(0).and(bits.significand.eq(0_u64))
    }

    pub(in crate::x87) fn special_exponent(&self) -> Val<I1> {
        self.class
            .as_ref()
            .map(|class| class.0.eq(Class::QuietNaN as u32))
            .unwrap_or_else(|| self.bits().exponent_field().eq(0x7fff))
    }

    pub(in crate::x87) fn nan(&self) -> Val<I1> {
        if let Some(class) = &self.class {
            return class.0.eq(Class::QuietNaN as u32);
        }
        self.special_exponent().and(
            self.bits()
                .significand
                .and(0x7fff_ffff_ffff_ffff_u64)
                .ne(0_u64),
        )
    }

    pub(in crate::x87) fn signaling_nan(&self) -> Val<I1> {
        if self.class.is_some() {
            return false.into();
        }
        self.nan()
            .and(self.bits().significand.and(1_u64 << 62).eq(0_u64))
    }

    pub(in crate::x87) fn infinity(&self) -> Val<I1> {
        if self.class.is_some() {
            return false.into();
        }
        self.special_exponent().and(
            self.bits()
                .significand
                .and(0x7fff_ffff_ffff_ffff_u64)
                .eq(0_u64),
        )
    }

    pub(in crate::x87) fn denormal(&self) -> Val<I1> {
        if self.class.is_some() {
            return false.into();
        }
        let bits = self.bits();
        bits.exponent_field().eq(0).and(bits.significand.ne(0_u64))
    }

    pub(in crate::x87) fn unsupported(&self) -> Val<I1> {
        if self.class.is_some() {
            return false.into();
        }
        let bits = self.bits();
        bits.exponent_field()
            .ne(0)
            .and(bits.significand.and(1_u64 << 63).eq(0_u64))
    }
}
