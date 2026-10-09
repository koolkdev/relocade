//! Lowering pure scalar expressions to their Wasm carrier operations.
use super::Writer;
use wasm_encoder::Instruction;

mod wide;

use crate::{
    bitwise::BitwiseOp,
    floating,
    integer::{low_mask, BinaryOp, BitCountOp, CompareOp, RotateOp, ShiftOp},
    Expression, Type,
};

impl Writer<'_> {
    /// Operands are already on the stack in Wasm order. Their logical types select
    /// the instructions independently of the expression's result type. Conversions
    /// within one Wasm carrier have already been bypassed by representation selection.
    pub(super) fn expression(&mut self, result_type: Type, expression: Expression<Type>) {
        let wide = result_type == Type::I64;
        let instruction = match expression {
            Expression::FloatBinary { operator, .. } => match operator {
                floating::BinaryOp::Add => Instruction::F64Add,
                floating::BinaryOp::Sub => Instruction::F64Sub,
                floating::BinaryOp::Mul => Instruction::F64Mul,
                floating::BinaryOp::Div => Instruction::F64Div,
            },
            Expression::FloatUnary { operator, .. } => match operator {
                floating::UnaryOp::Abs => Instruction::F64Abs,
                floating::UnaryOp::Neg => Instruction::F64Neg,
            },
            Expression::FloatCompare { operator, .. } => match operator {
                floating::CompareOp::Eq => Instruction::F64Eq,
                floating::CompareOp::Ne => Instruction::F64Ne,
                floating::CompareOp::Lt => Instruction::F64Lt,
                floating::CompareOp::Le => Instruction::F64Le,
                floating::CompareOp::Gt => Instruction::F64Gt,
                floating::CompareOp::Ge => Instruction::F64Ge,
            },
            Expression::Reinterpret { input } => match (input, result_type) {
                (Type::I64, Type::F64) => Instruction::F64ReinterpretI64,
                (Type::F64, Type::I64) => Instruction::I64ReinterpretF64,
                _ => unreachable!("bitcasts preserve width and change scalar type"),
            },
            Expression::Bitwise { operator, .. } => match (operator, result_type.carrier()) {
                (BitwiseOp::And, Type::I32) => Instruction::I32And,
                (BitwiseOp::And, Type::I64) => Instruction::I64And,
                (BitwiseOp::Or, Type::I32) => Instruction::I32Or,
                (BitwiseOp::Or, Type::I64) => Instruction::I64Or,
                (BitwiseOp::Xor, Type::I32) => Instruction::I32Xor,
                (BitwiseOp::Xor, Type::I64) => Instruction::I64Xor,
                _ => unreachable!("bitwise operations require integer carriers"),
            },
            Expression::Binary { operator, .. } => match (operator, wide) {
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
            Expression::MultiplyWide { signed, .. } => {
                if self.features.wide_arithmetic {
                    if signed {
                        Instruction::I64MulWideS
                    } else {
                        Instruction::I64MulWideU
                    }
                } else {
                    self.multiply_wide(signed);
                    return;
                }
            }
            Expression::Shift { operator, .. } => match (operator, wide) {
                (ShiftOp::Left, false) => Instruction::I32Shl,
                (ShiftOp::Left, true) => Instruction::I64Shl,
                (ShiftOp::RightUnsigned, false) => Instruction::I32ShrU,
                (ShiftOp::RightUnsigned, true) => Instruction::I64ShrU,
                (ShiftOp::RightSigned, false) => Instruction::I32ShrS,
                (ShiftOp::RightSigned, true) => Instruction::I64ShrS,
            },
            Expression::Rotate { operator, .. } => match (operator, wide) {
                (RotateOp::Left, false) => Instruction::I32Rotl,
                (RotateOp::Left, true) => Instruction::I64Rotl,
                (RotateOp::Right, false) => Instruction::I32Rotr,
                (RotateOp::Right, true) => Instruction::I64Rotr,
            },
            Expression::Select { .. } => Instruction::Select,
            Expression::BitCount { operator, .. } => {
                match (operator, wide) {
                    (BitCountOp::Ones, false) => Instruction::I32Popcnt,
                    (BitCountOp::Ones, true) => Instruction::I64Popcnt,
                    (BitCountOp::LeadingZeros, true) => Instruction::I64Clz,
                    (BitCountOp::TrailingZeros, true) => Instruction::I64Ctz,
                    (BitCountOp::LeadingZeros, false) => {
                        let padding = 32 - result_type.bits();
                        if padding == 0 {
                            Instruction::I32Clz
                        } else {
                            self.emit(Instruction::I32Clz);
                            self.emit(Instruction::I32Const(i32::from(padding)));
                            Instruction::I32Sub
                        }
                    }
                    (BitCountOp::TrailingZeros, false) => {
                        if result_type.bits() < 32 {
                            // The first bit above the logical value caps the zero case
                            // at its width without changing any nonzero count.
                            self.emit(Instruction::I32Const(1 << result_type.bits()));
                            self.emit(Instruction::I32Or);
                        }
                        Instruction::I32Ctz
                    }
                }
            }
            Expression::SignExtend { input } => {
                match input {
                    Type::I1 => {
                        self.emit(Instruction::I32Const(31));
                        self.emit(Instruction::I32Shl);
                        self.emit(Instruction::I32Const(31));
                        self.emit(Instruction::I32ShrS);
                    }
                    Type::I8 => self.emit(Instruction::I32Extend8S),
                    Type::I16 => self.emit(Instruction::I32Extend16S),
                    Type::I32 => {}
                    Type::I64 | Type::F64 => {
                        unreachable!("a signed extension widens an integer input")
                    }
                }
                if wide {
                    Instruction::I64ExtendI32S
                } else {
                    return;
                }
            }
            Expression::Compare { operator, left, .. } => {
                // Comparisons produce I1; their opcode follows the operands' carrier.
                let wide = left == Type::I64;
                match (operator, wide) {
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
                }
            }
            Expression::ZeroTest { input, nonzero } => {
                let test = if input == Type::I64 {
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
            Expression::LowBits { bits, .. } => {
                if wide {
                    self.emit(Instruction::I64Const(low_mask(bits) as i64));
                    Instruction::I64And
                } else {
                    self.emit(Instruction::I32Const(low_mask(bits) as i32));
                    Instruction::I32And
                }
            }
            Expression::Convert { .. } => {
                if wide {
                    Instruction::I64ExtendI32U
                } else {
                    Instruction::I32WrapI64
                }
            }
        };
        self.emit(instruction);
    }
}
