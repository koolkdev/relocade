//! Wasm lowering for opcodes, preserving logical input and result widths.
use super::{vector, Writer};
use crate::{
    bitwise::BitwiseOp,
    expression::{
        BinaryOp as Binary, CompareOp as Compare, ShiftOp as Shift, TernaryOp as Ternary,
        UnaryOp as Unary,
    },
    floating,
    integer::{low_mask, BinaryOp, BitCountOp, CompareOp, RotateOp, ShiftOp},
    Type,
};
use wasm_encoder::Instruction;

impl Writer<'_> {
    pub(super) fn unary(&mut self, operator: Unary, input_type: Type, ty: Type) {
        let wide = ty == Type::I64;
        let instruction = match operator {
            Unary::Bitmask { lane_type } => vector::bitmask(lane_type),
            Unary::Splat { lane_type } => vector::splat(lane_type),
            Unary::ExtractLane { lane } => vector::extract(ty, lane),
            Unary::Reinterpret => match (input_type.carrier(), ty.carrier()) {
                (Type::I64, Type::F64) => Instruction::F64ReinterpretI64,
                (Type::F64, Type::I64) => Instruction::I64ReinterpretF64,
                (Type::I64, Type::V128) => Instruction::I64x2Splat,
                (Type::V128, Type::I64) => Instruction::I64x2ExtractLane(0),
                _ => unreachable!("bitcasts preserve logical bits across carriers"),
            },
            Unary::Convert => {
                if wide {
                    Instruction::I64ExtendI32U
                } else {
                    Instruction::I32WrapI64
                }
            }
            Unary::SignExtend { input_type } => {
                match input_type {
                    Type::I1 => {
                        self.emit(Instruction::I32Const(31));
                        self.emit(Instruction::I32Shl);
                        self.emit(Instruction::I32Const(31));
                        self.emit(Instruction::I32ShrS);
                    }
                    Type::I8 => self.emit(Instruction::I32Extend8S),
                    Type::I16 => self.emit(Instruction::I32Extend16S),
                    Type::I32 => {}
                    Type::I64 | Type::F64 | Type::V64 | Type::V128 => {
                        unreachable!("a signed extension widens an integer input")
                    }
                }
                if wide {
                    Instruction::I64ExtendI32S
                } else {
                    return;
                }
            }
            Unary::ZeroTest { nonzero } => {
                let test = if input_type == Type::I64 {
                    Instruction::I64Eqz
                } else {
                    Instruction::I32Eqz
                };
                if nonzero {
                    self.emit(test);
                    Instruction::I32Eqz
                } else {
                    test
                }
            }
            Unary::LowBits { bits } => {
                if wide {
                    self.emit(Instruction::I64Const(low_mask(bits) as i64));
                    Instruction::I64And
                } else {
                    self.emit(Instruction::I32Const(low_mask(bits) as i32));
                    Instruction::I32And
                }
            }
            Unary::Float(operator) => match operator {
                floating::UnaryOp::Abs => Instruction::F64Abs,
                floating::UnaryOp::Neg => Instruction::F64Neg,
            },
            Unary::BitCount {
                operator,
                input_type,
            } => match (operator, input_type) {
                (BitCountOp::Ones, Type::I64) => Instruction::I64Popcnt,
                (BitCountOp::Ones, _) => Instruction::I32Popcnt,
                (BitCountOp::LeadingZeros, Type::I32) => Instruction::I32Clz,
                (BitCountOp::LeadingZeros, Type::I64) => Instruction::I64Clz,
                (BitCountOp::TrailingZeros, Type::I32) => Instruction::I32Ctz,
                (BitCountOp::TrailingZeros, Type::I64) => Instruction::I64Ctz,
                _ => unreachable!("narrow counts are lowered before placement"),
            },
        };
        self.emit(instruction);
    }
}

pub(super) fn binary(operator: Binary, input_type: Type) -> Instruction<'static> {
    match operator {
        Binary::Compare(operator) => compare(operator, input_type),
        Binary::Bitwise(operator) => bitwise(operator, input_type),
        Binary::Swizzle {
            input_type: Type::V128,
        } => Instruction::I8x16Swizzle,
        Binary::Swizzle { .. } => unreachable!("logical swizzles lower before placement"),
        Binary::ShuffleImmediate(lanes) => Instruction::I8x16Shuffle(lanes.indices()),
        Binary::ReplaceLane { lane_type, lane } => vector::replace(lane_type, lane),
        Binary::Integer(operator) => match (operator, input_type == Type::I64) {
            (BinaryOp::Add, false) => Instruction::I32Add,
            (BinaryOp::Add, true) => Instruction::I64Add,
            (BinaryOp::Sub, false) => Instruction::I32Sub,
            (BinaryOp::Sub, true) => Instruction::I64Sub,
            (BinaryOp::Mul, false) => Instruction::I32Mul,
            (BinaryOp::Mul, true) => Instruction::I64Mul,
            (BinaryOp::DivUnsigned, false) => Instruction::I32DivU,
            (BinaryOp::DivUnsigned, true) => Instruction::I64DivU,
            (BinaryOp::DivSigned, false) => Instruction::I32DivS,
            (BinaryOp::DivSigned, true) => Instruction::I64DivS,
            (BinaryOp::RemUnsigned, false) => Instruction::I32RemU,
            (BinaryOp::RemUnsigned, true) => Instruction::I64RemU,
            (BinaryOp::RemSigned, false) => Instruction::I32RemS,
            (BinaryOp::RemSigned, true) => Instruction::I64RemS,
        },
        Binary::Float(operator) => match operator {
            floating::BinaryOp::Add => Instruction::F64Add,
            floating::BinaryOp::Sub => Instruction::F64Sub,
            floating::BinaryOp::Mul => Instruction::F64Mul,
            floating::BinaryOp::Div => Instruction::F64Div,
        },
        Binary::Vector {
            operator,
            lane_type,
        } => vector::binary(operator, lane_type),
        Binary::Narrow {
            signed,
            result_lane,
        } => vector::narrow(signed, result_lane),
    }
}

pub(super) fn ternary(operator: Ternary) -> Instruction<'static> {
    match operator {
        Ternary::Select => Instruction::Select,
        Ternary::Bitselect => Instruction::V128Bitselect,
    }
}

fn compare(operator: Compare, input_type: Type) -> Instruction<'static> {
    // Comparisons produce I1; their opcode follows the operands' carrier.
    match operator {
        Compare::Integer(operator) => match (operator, input_type == Type::I64) {
            (CompareOp::Eq, false) => Instruction::I32Eq,
            (CompareOp::Eq, true) => Instruction::I64Eq,
            (CompareOp::Ne, false) => Instruction::I32Ne,
            (CompareOp::Ne, true) => Instruction::I64Ne,
            (CompareOp::LtUnsigned, false) => Instruction::I32LtU,
            (CompareOp::LtUnsigned, true) => Instruction::I64LtU,
            (CompareOp::GeUnsigned, false) => Instruction::I32GeU,
            (CompareOp::GeUnsigned, true) => Instruction::I64GeU,
            (CompareOp::LtSigned, false) => Instruction::I32LtS,
            (CompareOp::LtSigned, true) => Instruction::I64LtS,
            (CompareOp::GeSigned, false) => Instruction::I32GeS,
            (CompareOp::GeSigned, true) => Instruction::I64GeS,
        },
        Compare::Float(operator) => match operator {
            floating::CompareOp::Eq => Instruction::F64Eq,
            floating::CompareOp::Ne => Instruction::F64Ne,
            floating::CompareOp::Lt => Instruction::F64Lt,
            floating::CompareOp::Le => Instruction::F64Le,
            floating::CompareOp::Gt => Instruction::F64Gt,
            floating::CompareOp::Ge => Instruction::F64Ge,
        },
    }
}

pub(super) fn shift(operator: Shift, ty: Type) -> Instruction<'static> {
    match operator {
        Shift::Rotate(operator) => rotate(operator, ty),
        Shift::Scalar(operator) => match (operator, ty == Type::I64) {
            (ShiftOp::Left, false) => Instruction::I32Shl,
            (ShiftOp::Left, true) => Instruction::I64Shl,
            (ShiftOp::RightUnsigned, false) => Instruction::I32ShrU,
            (ShiftOp::RightUnsigned, true) => Instruction::I64ShrU,
            (ShiftOp::RightSigned, false) => Instruction::I32ShrS,
            (ShiftOp::RightSigned, true) => Instruction::I64ShrS,
        },
        Shift::Lanes {
            operator,
            lane_type,
        } => vector::shift(operator, lane_type),
    }
}

fn rotate(operator: RotateOp, ty: Type) -> Instruction<'static> {
    match (operator, ty == Type::I64) {
        (RotateOp::Left, false) => Instruction::I32Rotl,
        (RotateOp::Left, true) => Instruction::I64Rotl,
        (RotateOp::Right, false) => Instruction::I32Rotr,
        (RotateOp::Right, true) => Instruction::I64Rotr,
    }
}

fn bitwise(operator: BitwiseOp, ty: Type) -> Instruction<'static> {
    match (operator, ty.carrier()) {
        (BitwiseOp::And, Type::I32) => Instruction::I32And,
        (BitwiseOp::And, Type::I64) => Instruction::I64And,
        (BitwiseOp::And, Type::V128) => Instruction::V128And,
        (BitwiseOp::Or, Type::I32) => Instruction::I32Or,
        (BitwiseOp::Or, Type::I64) => Instruction::I64Or,
        (BitwiseOp::Or, Type::V128) => Instruction::V128Or,
        (BitwiseOp::Xor, Type::I32) => Instruction::I32Xor,
        (BitwiseOp::Xor, Type::I64) => Instruction::I64Xor,
        (BitwiseOp::Xor, Type::V128) => Instruction::V128Xor,
        _ => unreachable!("bitwise operations require integer or vector carriers"),
    }
}
