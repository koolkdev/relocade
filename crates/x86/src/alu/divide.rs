//! Integer division values and numeric conditions for guest divide error.

use wasm86_compiler::{Val, I1};

use super::DoubleWidth;

#[derive(Clone, Copy)]
pub(crate) enum DivideOp {
    Unsigned,
    Signed,
}

pub(crate) struct DivisionResult<T: DoubleWidth> {
    pub(crate) quotient: Val<T>,
    pub(crate) remainder: Val<T>,
    /// An additional quotient-fit check when input validation does not prove it.
    pub(crate) overflow: Option<Val<I1>>,
}

impl DivideOp {
    pub(crate) fn input_fault<T: DoubleWidth>(
        self,
        dividend: &Val<T::Double>,
        divisor: &Val<T>,
    ) -> Val<I1> {
        let width = T::BYTES * 8;
        match self {
            // H*2^width + L fits a width-bit quotient exactly when H < divisor.
            // This also rejects a zero divisor before division.
            Self::Unsigned => dividend
                .unsigned()
                .shr(width)
                .truncate::<T>()
                .unsigned()
                .ge(divisor),
            Self::Signed => {
                let zero = divisor.eq(0);
                if T::BYTES == 1 {
                    // The I16 quotient for -32768/-1 wraps to 0x8000, which still
                    // fails the later signed-byte fit check.
                    zero
                } else {
                    let minimum = Val::<T::Double>::from(1).shl(width * 2 - 1);
                    zero.or(dividend.eq(minimum).and(divisor.eq(-1)))
                }
            }
        }
    }

    /// Guard `input_fault` before consuming these expressions, then reject any
    /// returned overflow before defining architectural results. Construction
    /// only creates values; it does not execute arithmetic or publish faults.
    pub(crate) fn apply<T: DoubleWidth>(
        self,
        dividend: Val<T::Double>,
        divisor: Val<T>,
    ) -> DivisionResult<T> {
        let divisor = match self {
            Self::Unsigned => divisor.unsigned().extend::<T::Double>(),
            Self::Signed => divisor.signed().extend::<T::Double>(),
        };
        let quotient = match self {
            Self::Unsigned => dividend.unsigned().div(&divisor),
            Self::Signed => dividend.signed().div(&divisor),
        };
        let overflow = match self {
            Self::Unsigned => None,
            Self::Signed => {
                Some(quotient.ne(quotient.truncate::<T>().signed().extend::<T::Double>()))
            }
        };
        let remainder = match self {
            Self::Unsigned => dividend.unsigned().rem(&divisor),
            Self::Signed => dividend.signed().rem(&divisor),
        };
        DivisionResult {
            quotient: quotient.truncate::<T>(),
            remainder: remainder.truncate::<T>(),
            overflow,
        }
    }
}
