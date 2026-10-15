//! Direct strict floating SIMD primitives; semantic policies stay with consumers.

use crate::floating::{BinaryOp, CompareOp, Format, UnaryOp, VectorConvertOp};
use wasm_encoder::Instruction;

pub(super) fn binary(format: Format, operator: BinaryOp) -> Instruction<'static> {
    match (format, operator) {
        (Format::Binary32, BinaryOp::Add) => Instruction::F32x4Add,
        (Format::Binary32, BinaryOp::Sub) => Instruction::F32x4Sub,
        (Format::Binary32, BinaryOp::Mul) => Instruction::F32x4Mul,
        (Format::Binary32, BinaryOp::Div) => Instruction::F32x4Div,
        (Format::Binary64, BinaryOp::Add) => Instruction::F64x2Add,
        (Format::Binary64, BinaryOp::Sub) => Instruction::F64x2Sub,
        (Format::Binary64, BinaryOp::Mul) => Instruction::F64x2Mul,
        (Format::Binary64, BinaryOp::Div) => Instruction::F64x2Div,
    }
}

pub(super) fn unary(format: Format, operator: UnaryOp) -> Instruction<'static> {
    match (format, operator) {
        (Format::Binary32, UnaryOp::Abs) => Instruction::F32x4Abs,
        (Format::Binary32, UnaryOp::Neg) => Instruction::F32x4Neg,
        (Format::Binary32, UnaryOp::Ceil) => Instruction::F32x4Ceil,
        (Format::Binary32, UnaryOp::Floor) => Instruction::F32x4Floor,
        (Format::Binary32, UnaryOp::Trunc) => Instruction::F32x4Trunc,
        (Format::Binary32, UnaryOp::Nearest) => Instruction::F32x4Nearest,
        (Format::Binary32, UnaryOp::Sqrt) => Instruction::F32x4Sqrt,
        (Format::Binary64, UnaryOp::Abs) => Instruction::F64x2Abs,
        (Format::Binary64, UnaryOp::Neg) => Instruction::F64x2Neg,
        (Format::Binary64, UnaryOp::Ceil) => Instruction::F64x2Ceil,
        (Format::Binary64, UnaryOp::Floor) => Instruction::F64x2Floor,
        (Format::Binary64, UnaryOp::Trunc) => Instruction::F64x2Trunc,
        (Format::Binary64, UnaryOp::Nearest) => Instruction::F64x2Nearest,
        (Format::Binary64, UnaryOp::Sqrt) => Instruction::F64x2Sqrt,
    }
}

pub(super) fn compare(format: Format, operator: CompareOp) -> Instruction<'static> {
    match (format, operator) {
        (Format::Binary32, CompareOp::Eq) => Instruction::F32x4Eq,
        (Format::Binary32, CompareOp::Ne) => Instruction::F32x4Ne,
        (Format::Binary32, CompareOp::Lt) => Instruction::F32x4Lt,
        (Format::Binary32, CompareOp::Le) => Instruction::F32x4Le,
        (Format::Binary32, CompareOp::Gt) => Instruction::F32x4Gt,
        (Format::Binary32, CompareOp::Ge) => Instruction::F32x4Ge,
        (Format::Binary64, CompareOp::Eq) => Instruction::F64x2Eq,
        (Format::Binary64, CompareOp::Ne) => Instruction::F64x2Ne,
        (Format::Binary64, CompareOp::Lt) => Instruction::F64x2Lt,
        (Format::Binary64, CompareOp::Le) => Instruction::F64x2Le,
        (Format::Binary64, CompareOp::Gt) => Instruction::F64x2Gt,
        (Format::Binary64, CompareOp::Ge) => Instruction::F64x2Ge,
    }
}

pub(super) fn convert(operator: VectorConvertOp) -> Instruction<'static> {
    match operator {
        VectorConvertOp::PromoteLowBinary32 => Instruction::F64x2PromoteLowF32x4,
        VectorConvertOp::DemoteBinary64Zero => Instruction::F32x4DemoteF64x2Zero,
        VectorConvertOp::FromSignedI32 {
            format: Format::Binary32,
        } => Instruction::F32x4ConvertI32x4S,
        VectorConvertOp::FromSignedI32 {
            format: Format::Binary64,
        } => Instruction::F64x2ConvertLowI32x4S,
        VectorConvertOp::TruncateSaturatingI32 {
            format: Format::Binary32,
        } => Instruction::I32x4TruncSatF32x4S,
        VectorConvertOp::TruncateSaturatingI32 {
            format: Format::Binary64,
        } => Instruction::I32x4TruncSatF64x2SZero,
    }
}
