//! Wasm SIMD encodings for logical integer lanes.

use crate::{expression::LaneBinaryOp, integer::ShiftOp, Type};
use wasm_encoder::Instruction;

pub(super) fn binary(operator: LaneBinaryOp, lane_type: Type) -> Instruction<'static> {
    match (operator, lane_type) {
        (LaneBinaryOp::Add, Type::I8) => Instruction::I8x16Add,
        (LaneBinaryOp::Add, Type::I16) => Instruction::I16x8Add,
        (LaneBinaryOp::Add, Type::I32) => Instruction::I32x4Add,
        (LaneBinaryOp::Add, Type::I64) => Instruction::I64x2Add,
        (LaneBinaryOp::Sub, Type::I8) => Instruction::I8x16Sub,
        (LaneBinaryOp::Sub, Type::I16) => Instruction::I16x8Sub,
        (LaneBinaryOp::Sub, Type::I32) => Instruction::I32x4Sub,
        (LaneBinaryOp::Sub, Type::I64) => Instruction::I64x2Sub,
        (LaneBinaryOp::Mul, Type::I16) => Instruction::I16x8Mul,
        (LaneBinaryOp::Mul, Type::I32) => Instruction::I32x4Mul,
        (LaneBinaryOp::Mul, Type::I64) => Instruction::I64x2Mul,
        (LaneBinaryOp::WideningMulLowSigned, Type::I8) => Instruction::I16x8ExtMulLowI8x16S,
        (LaneBinaryOp::WideningMulLowSigned, Type::I16) => Instruction::I32x4ExtMulLowI16x8S,
        (LaneBinaryOp::WideningMulLowSigned, Type::I32) => Instruction::I64x2ExtMulLowI32x4S,
        (LaneBinaryOp::DotI16Signed, Type::I16) => Instruction::I32x4DotI16x8S,
        (LaneBinaryOp::AddSatSigned, Type::I8) => Instruction::I8x16AddSatS,
        (LaneBinaryOp::AddSatSigned, Type::I16) => Instruction::I16x8AddSatS,
        (LaneBinaryOp::AddSatUnsigned, Type::I8) => Instruction::I8x16AddSatU,
        (LaneBinaryOp::AddSatUnsigned, Type::I16) => Instruction::I16x8AddSatU,
        (LaneBinaryOp::SubSatSigned, Type::I8) => Instruction::I8x16SubSatS,
        (LaneBinaryOp::SubSatSigned, Type::I16) => Instruction::I16x8SubSatS,
        (LaneBinaryOp::SubSatUnsigned, Type::I8) => Instruction::I8x16SubSatU,
        (LaneBinaryOp::SubSatUnsigned, Type::I16) => Instruction::I16x8SubSatU,
        (LaneBinaryOp::Eq, Type::I8) => Instruction::I8x16Eq,
        (LaneBinaryOp::Eq, Type::I16) => Instruction::I16x8Eq,
        (LaneBinaryOp::Eq, Type::I32) => Instruction::I32x4Eq,
        (LaneBinaryOp::Eq, Type::I64) => Instruction::I64x2Eq,
        (LaneBinaryOp::GtSigned, Type::I8) => Instruction::I8x16GtS,
        (LaneBinaryOp::GtSigned, Type::I16) => Instruction::I16x8GtS,
        (LaneBinaryOp::GtSigned, Type::I32) => Instruction::I32x4GtS,
        (LaneBinaryOp::GtSigned, Type::I64) => Instruction::I64x2GtS,
        _ => unreachable!("the vector API restricts operations to supported lane widths"),
    }
}

pub(super) fn shift(operator: ShiftOp, lane_type: Type) -> Instruction<'static> {
    match (operator, lane_type) {
        (ShiftOp::Left, Type::I8) => Instruction::I8x16Shl,
        (ShiftOp::Left, Type::I16) => Instruction::I16x8Shl,
        (ShiftOp::Left, Type::I32) => Instruction::I32x4Shl,
        (ShiftOp::Left, Type::I64) => Instruction::I64x2Shl,
        (ShiftOp::RightUnsigned, Type::I8) => Instruction::I8x16ShrU,
        (ShiftOp::RightUnsigned, Type::I16) => Instruction::I16x8ShrU,
        (ShiftOp::RightUnsigned, Type::I32) => Instruction::I32x4ShrU,
        (ShiftOp::RightUnsigned, Type::I64) => Instruction::I64x2ShrU,
        (ShiftOp::RightSigned, Type::I8) => Instruction::I8x16ShrS,
        (ShiftOp::RightSigned, Type::I16) => Instruction::I16x8ShrS,
        (ShiftOp::RightSigned, Type::I32) => Instruction::I32x4ShrS,
        (ShiftOp::RightSigned, Type::I64) => Instruction::I64x2ShrS,
        _ => unreachable!("integer lanes have 8, 16, 32 or 64 bits"),
    }
}

pub(super) fn extract(lane_type: Type, lane: u8) -> Instruction<'static> {
    match lane_type {
        Type::I8 => Instruction::I8x16ExtractLaneU(lane),
        Type::I16 => Instruction::I16x8ExtractLaneU(lane),
        Type::I32 => Instruction::I32x4ExtractLane(lane),
        Type::I64 => Instruction::I64x2ExtractLane(lane),
        _ => unreachable!("integer lanes have 8, 16, 32 or 64 bits"),
    }
}

pub(super) fn replace(lane_type: Type, lane: u8) -> Instruction<'static> {
    match lane_type {
        Type::I8 => Instruction::I8x16ReplaceLane(lane),
        Type::I16 => Instruction::I16x8ReplaceLane(lane),
        Type::I32 => Instruction::I32x4ReplaceLane(lane),
        Type::I64 => Instruction::I64x2ReplaceLane(lane),
        _ => unreachable!("integer lanes have 8, 16, 32 or 64 bits"),
    }
}

pub(super) fn narrow(signed: bool, result_lane: Type) -> Instruction<'static> {
    match (signed, result_lane) {
        (true, Type::I8) => Instruction::I8x16NarrowI16x8S,
        (false, Type::I8) => Instruction::I8x16NarrowI16x8U,
        (true, Type::I16) => Instruction::I16x8NarrowI32x4S,
        (false, Type::I16) => Instruction::I16x8NarrowI32x4U,
        _ => unreachable!("narrowing produces byte or word lanes"),
    }
}
