//! Lazy numerical predicates over exact encodings or an established class.

use wasm86_compiler::{Val, I1, I16, I64, I8};

use super::{BinaryFormat, ExtendedBits};

#[cfg(test)]
mod tests;

enum Class {
    Normal = 0,
    Zero = 1,
    QuietNaN = 2,
}

/// An exact class established by construction or a successful guard.
/// These three classes use their architectural tag codes. The special code
/// specifically proves a quiet NaN, not an arbitrary special encoding.
#[derive(Clone)]
pub(in crate::x87) struct KnownClass(Val<I8>);

impl KnownClass {
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

/// A borrowed view builds only the requested predicates. Precision evidence
/// alone does not establish a class or restrict the extended exponent range.
pub(crate) struct Classification<'a> {
    source: Source<'a>,
}

enum Source<'a> {
    Extended(&'a ExtendedBits),
    LoadedBinary(BinaryFormat, &'a Val<I64>),
    Known(&'a KnownClass),
}

// These predicates can overlap for unsupported extended encodings. In
// particular, a special exponent with a missing integer bit is not an IEEE class.
#[derive(Clone, Copy)]
enum Predicate {
    Normal,
    NormalOrZero,
    Zero,
    SpecialExponent,
    Infinity,
    NaN,
    SignalingNaN,
    Denormal,
    Unsupported,
}

impl<'a> Classification<'a> {
    pub(super) fn known(class: &'a KnownClass) -> Self {
        Self {
            source: Source::Known(class),
        }
    }

    pub(super) fn extended(bits: &'a ExtendedBits) -> Self {
        Self {
            source: Source::Extended(bits),
        }
    }

    pub(super) fn loaded_binary(format: BinaryFormat, bits: &'a Val<I64>) -> Self {
        Self {
            source: Source::LoadedBinary(format, bits),
        }
    }

    fn test(&self, predicate: Predicate) -> Val<I1> {
        match self.source {
            Source::Known(class) => match predicate {
                Predicate::Normal => class.0.eq(Class::Normal as u32),
                Predicate::NormalOrZero => class.0.ne(Class::QuietNaN as u32),
                Predicate::Zero => class.0.eq(Class::Zero as u32),
                Predicate::SpecialExponent | Predicate::NaN => class.0.eq(Class::QuietNaN as u32),
                Predicate::Infinity
                | Predicate::SignalingNaN
                | Predicate::Denormal
                | Predicate::Unsupported => false.into(),
            },
            Source::LoadedBinary(format, bits) => {
                // Loading normalizes narrow subnormals and quiets NaNs. These
                // guarantees belong to the loaded value, not the source operand.
                let class = format.classification(bits);
                match predicate {
                    Predicate::Normal => class.zero().eq(false).and(class.finite()),
                    Predicate::NormalOrZero => class.finite(),
                    Predicate::Zero => class.zero(),
                    Predicate::SpecialExponent => class.finite().eq(false),
                    Predicate::Infinity => class.infinity(),
                    Predicate::NaN => class.nan(),
                    Predicate::SignalingNaN | Predicate::Denormal | Predicate::Unsupported => {
                        false.into()
                    }
                }
            }
            Source::Extended(bits) => {
                let exponent = || bits.exponent_field();
                match predicate {
                    Predicate::Normal => {
                        let exponent = exponent();
                        exponent
                            .ne(0)
                            .and(exponent.ne(0x7fff))
                            .and(bits.significand.and(1_u64 << 63).ne(0_u64))
                    }
                    Predicate::NormalOrZero => self.normal().or(self.zero()),
                    Predicate::Zero => exponent().eq(0).and(bits.significand.eq(0_u64)),
                    Predicate::SpecialExponent => exponent().eq(0x7fff),
                    Predicate::Infinity => self
                        .special_exponent()
                        .and(bits.significand.and(0x7fff_ffff_ffff_ffff_u64).eq(0_u64)),
                    Predicate::NaN => self
                        .special_exponent()
                        .and(bits.significand.and(0x7fff_ffff_ffff_ffff_u64).ne(0_u64)),
                    Predicate::SignalingNaN => {
                        self.nan().and(bits.significand.and(1_u64 << 62).eq(0_u64))
                    }
                    Predicate::Denormal => exponent().eq(0).and(bits.significand.ne(0_u64)),
                    Predicate::Unsupported => exponent()
                        .ne(0)
                        .and(bits.significand.and(1_u64 << 63).eq(0_u64)),
                }
            }
        }
    }

    pub(in crate::x87) fn normal(&self) -> Val<I1> {
        self.test(Predicate::Normal)
    }

    pub(in crate::x87) fn normal_or_zero(&self) -> Val<I1> {
        self.test(Predicate::NormalOrZero)
    }

    pub(in crate::x87) fn zero(&self) -> Val<I1> {
        self.test(Predicate::Zero)
    }

    pub(in crate::x87) fn special_exponent(&self) -> Val<I1> {
        self.test(Predicate::SpecialExponent)
    }

    pub(in crate::x87) fn infinity(&self) -> Val<I1> {
        self.test(Predicate::Infinity)
    }

    pub(in crate::x87) fn nan(&self) -> Val<I1> {
        self.test(Predicate::NaN)
    }

    pub(in crate::x87) fn signaling_nan(&self) -> Val<I1> {
        self.test(Predicate::SignalingNaN)
    }

    pub(in crate::x87) fn denormal(&self) -> Val<I1> {
        self.test(Predicate::Denormal)
    }

    pub(in crate::x87) fn unsupported(&self) -> Val<I1> {
        self.test(Predicate::Unsupported)
    }

    pub(crate) fn tag(&self) -> Val<I16> {
        if let Source::Known(class) = self.source {
            return class.0.unsigned().extend::<I16>();
        }
        if let Source::LoadedBinary(format, bits) = self.source {
            let class = format.classification(bits);
            return class
                .zero()
                .select(1_u32, class.special_exponent().select(2_u32, 0_u32));
        }
        self.zero()
            .select(1_u32, self.normal().select(0_u32, 2_u32))
    }
}
