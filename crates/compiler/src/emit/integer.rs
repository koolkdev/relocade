//! Lowering logical integer expressions to Wasm carrier operations.
use wasm_encoder::Instruction;

use crate::{
    integer::{low_mask, BinaryOp, BitCountOp, CompareOp, RotateOp, ShiftOp},
    Expression, Type,
};

/// Operands are already on the stack in Wasm order. Their logical types select
/// the instructions independently of the expression's result type. Conversions
/// within one Wasm carrier have already been bypassed by representation selection.
pub(super) fn lower(result_type: Type, expression: Expression<Type>) -> Vec<Instruction<'static>> {
    let mut code = Vec::new();
    let wide = result_type == Type::I64;
    let instruction = match expression {
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
            (BinaryOp::And, false) => Instruction::I32And,
            (BinaryOp::And, true) => Instruction::I64And,
            (BinaryOp::Or, false) => Instruction::I32Or,
            (BinaryOp::Or, true) => Instruction::I64Or,
            (BinaryOp::Xor, false) => Instruction::I32Xor,
            (BinaryOp::Xor, true) => Instruction::I64Xor,
        },
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
                        code.push(Instruction::I32Clz);
                        code.push(Instruction::I32Const(i32::from(padding)));
                        Instruction::I32Sub
                    }
                }
                (BitCountOp::TrailingZeros, false) => {
                    if result_type.bits() < 32 {
                        // The first bit above the logical value caps the zero case
                        // at its width without changing any nonzero count.
                        code.push(Instruction::I32Const(1 << result_type.bits()));
                        code.push(Instruction::I32Or);
                    }
                    Instruction::I32Ctz
                }
            }
        }
        Expression::SignExtend { input } => {
            match input {
                Type::I1 => {
                    code.push(Instruction::I32Const(31));
                    code.push(Instruction::I32Shl);
                    code.push(Instruction::I32Const(31));
                    code.push(Instruction::I32ShrS);
                }
                Type::I8 => code.push(Instruction::I32Extend8S),
                Type::I16 => code.push(Instruction::I32Extend16S),
                Type::I32 => {}
                Type::I64 => unreachable!("a signed extension widens its input"),
            }
            if wide {
                Instruction::I64ExtendI32S
            } else {
                return code;
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
                code.push(test);
                Instruction::I32Eqz
            } else {
                test
            }
        }
        Expression::LowBits { bits, .. } => {
            if wide {
                code.push(Instruction::I64Const(low_mask(bits) as i64));
                Instruction::I64And
            } else {
                code.push(Instruction::I32Const(low_mask(bits) as i32));
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
    code.push(instruction);
    code
}
