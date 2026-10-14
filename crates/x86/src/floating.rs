//! IEEE binary32/64 encoding helpers shared by x87 and SIMD.

mod classification;
mod finite;
mod rounding;

pub(crate) use classification::BinaryClassification;
pub(crate) use finite::{FiniteMagnitude, MagnitudeShape};
pub(crate) use rounding::{RoundingInput, RoundingMode};
use wasm86_compiler::{Val, VectorLane, F64, I32, I64};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum BinaryFormat {
    Binary32,
    Binary64,
}

impl BinaryFormat {
    pub(crate) fn exponent_field(self, bits: &Val<I64>) -> Val<I32> {
        bits.unsigned()
            .shr(self.fraction_bits())
            .truncate::<I32>()
            .and((1_u32 << self.exponent_bits()) - 1)
    }

    /// An exact numerical value for ordering. NaN payloads and signaling state
    /// remain in the original bits; native promotion need not preserve them.
    pub(crate) fn binary64_value(self, bits: &Val<I64>) -> Val<F64> {
        match self {
            Self::Binary32 => Val::<F64>::from_binary32_bits(bits.truncate::<I32>()),
            Self::Binary64 => Val::<F64>::from_bits(bits),
        }
    }

    pub(crate) fn fraction_bits(self) -> u32 {
        match self {
            Self::Binary32 => 23,
            Self::Binary64 => 52,
        }
    }

    pub(crate) fn exponent_bits(self) -> u32 {
        match self {
            Self::Binary32 => 8,
            Self::Binary64 => 11,
        }
    }

    pub(crate) fn bias(self) -> u32 {
        (1 << (self.exponent_bits() - 1)) - 1
    }

    pub(crate) fn infinity(self) -> u64 {
        ((1_u64 << self.exponent_bits()) - 1) << self.fraction_bits()
    }

    pub(crate) fn quiet_bit(self) -> u64 {
        1 << (self.fraction_bits() - 1)
    }

    pub(crate) fn sign_bit(self) -> u64 {
        1 << (self.fraction_bits() + self.exponent_bits())
    }

    pub(crate) fn indefinite_bits(self) -> u64 {
        self.sign_bit() | self.infinity() | self.quiet_bit()
    }

    pub(crate) fn bytes(self) -> u32 {
        match self {
            Self::Binary32 => 4,
            Self::Binary64 => 8,
        }
    }
}

/// The integer encoding of a binary32 or binary64 value.
pub(crate) trait BinaryEncoding: VectorLane {
    const FORMAT: BinaryFormat;
}

impl BinaryEncoding for I32 {
    const FORMAT: BinaryFormat = BinaryFormat::Binary32;
}

impl BinaryEncoding for I64 {
    const FORMAT: BinaryFormat = BinaryFormat::Binary64;
}
