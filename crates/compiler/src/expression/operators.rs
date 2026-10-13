//! Operation semantics, independent of the expression's operand storage.
//!
//! Integer facts apply only to Integer opcodes. Packed operations retain their
//! lane interpretation when operands are replaced or lowered to Wasm carriers.

use super::ShuffleMask;
use crate::{bitwise::BitwiseOp, floating, integer, Type};

/// One-input operations. The opcode defines the relation between logical input
/// and result types; carrier normalization does not erase lane widths.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum UnaryOp {
    Float(floating::UnaryOp),
    BitCount {
        operator: integer::BitCountOp,
        input_type: Type,
    },
    ExtractLane {
        lane: u8,
    },
    Bitmask {
        lane_type: Type,
    },
    Reinterpret,
    SignExtend {
        input_type: Type,
    },
    ZeroTest {
        nonzero: bool,
    },
    Convert,
    LowBits {
        bits: u8,
    },
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum BinaryOp {
    Bitwise(BitwiseOp),
    Compare(CompareOp),
    Shuffle(ShuffleMask),
    ReplaceLane {
        lane_type: Type,
        lane: u8,
    },
    Integer(integer::BinaryOp),
    Float(floating::BinaryOp),
    Vector {
        operator: LaneBinaryOp,
        // Input lane width; widening multiplication and dot products double it.
        lane_type: Type,
    },
    Narrow {
        signed: bool,
        result_lane: Type,
    },
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum CompareOp {
    Integer(integer::CompareOp),
    Float(floating::CompareOp),
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum ShiftOp {
    Scalar(integer::ShiftOp),
    Rotate(integer::RotateOp),
    Lanes {
        operator: integer::ShiftOp,
        lane_type: Type,
    },
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum LaneBinaryOp {
    Add,
    Sub,
    Mul,
    WideningMulLowSigned,
    WideningMulHighSigned,
    DotI16Signed,
    Eq,
    GtSigned,
    AddSatSigned,
    AddSatUnsigned,
    SubSatSigned,
    SubSatUnsigned,
}
