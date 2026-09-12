//! Lowering logical integer expressions to Wasm carrier operations.
use wasm_encoder::{Encode, Instruction};

use super::Scheduler;
use crate::{
    integer::{BinaryOp, BitCountOp, CompareOp, RotateOp, ShiftOp},
    Type, ValueKind,
};

impl Scheduler<'_> {
    pub(super) fn operation(&mut self, id: usize) {
        let value = self.body.values[id];
        let wide = value.ty == Type::I64;
        let instruction = match value.kind {
            ValueKind::Binary(operator, _, _) => match (operator, wide) {
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
            ValueKind::Shift { operator, .. } => match (operator, wide) {
                (ShiftOp::Left, false) => Instruction::I32Shl,
                (ShiftOp::Left, true) => Instruction::I64Shl,
                (ShiftOp::RightUnsigned, false) => Instruction::I32ShrU,
                (ShiftOp::RightUnsigned, true) => Instruction::I64ShrU,
                (ShiftOp::RightSigned, false) => Instruction::I32ShrS,
                (ShiftOp::RightSigned, true) => Instruction::I64ShrS,
            },
            ValueKind::Rotate { operator, .. } => match (operator, wide) {
                (RotateOp::Left, false) => Instruction::I32Rotl,
                (RotateOp::Left, true) => Instruction::I64Rotl,
                (RotateOp::Right, false) => Instruction::I32Rotr,
                (RotateOp::Right, true) => Instruction::I64Rotr,
            },
            ValueKind::Select { .. } => Instruction::Select,
            ValueKind::BitCount(operator, _) => match (operator, wide) {
                (BitCountOp::Ones, false) => Instruction::I32Popcnt,
                (BitCountOp::Ones, true) => Instruction::I64Popcnt,
                (BitCountOp::LeadingZeros, true) => Instruction::I64Clz,
                (BitCountOp::TrailingZeros, true) => Instruction::I64Ctz,
                (BitCountOp::LeadingZeros, false) => {
                    let padding = 32 - value.ty.bits();
                    if padding == 0 {
                        Instruction::I32Clz
                    } else {
                        Instruction::I32Clz.encode(&mut self.bytes);
                        Instruction::I32Const(i32::from(padding)).encode(&mut self.bytes);
                        Instruction::I32Sub
                    }
                }
                (BitCountOp::TrailingZeros, false) => {
                    if value.ty.bits() < 32 {
                        // The first bit above the logical value caps the zero case
                        // at its width without changing any nonzero count.
                        Instruction::I32Const(1 << value.ty.bits()).encode(&mut self.bytes);
                        Instruction::I32Or.encode(&mut self.bytes);
                    }
                    Instruction::I32Ctz
                }
            },
            ValueKind::SignExtend(input) => {
                match self.body.values[input].ty {
                    Type::I1 => {
                        Instruction::I32Const(31).encode(&mut self.bytes);
                        Instruction::I32Shl.encode(&mut self.bytes);
                        Instruction::I32Const(31).encode(&mut self.bytes);
                        Instruction::I32ShrS.encode(&mut self.bytes);
                    }
                    Type::I8 => Instruction::I32Extend8S.encode(&mut self.bytes),
                    Type::I16 => Instruction::I32Extend16S.encode(&mut self.bytes),
                    Type::I32 => {}
                    Type::I64 => unreachable!("a signed extension widens its input"),
                }
                if wide {
                    Instruction::I64ExtendI32S
                } else {
                    return;
                }
            }
            ValueKind::Compare(operator, a, _) => {
                // Comparisons produce I1; their opcode follows the operands' carrier.
                let wide = self.body.values[a].ty == Type::I64;
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
            ValueKind::ZeroTest { input, nonzero } => {
                let test = if self.body.values[input].ty == Type::I64 {
                    Instruction::I64Eqz
                } else {
                    Instruction::I32Eqz
                };
                if nonzero {
                    test.encode(&mut self.bytes);
                    Instruction::I32Eqz
                } else {
                    test
                }
            }
            ValueKind::Normalize(_) => {
                Instruction::I32Const(value.ty.mask() as i32).encode(&mut self.bytes);
                Instruction::I32And
            }
            ValueKind::Convert(_) => {
                if wide {
                    Instruction::I64ExtendI32U
                } else {
                    Instruction::I32WrapI64
                }
            }
            ValueKind::Constant(_)
            | ValueKind::Parameter(_)
            | ValueKind::LoopInput { .. }
            | ValueKind::Load { .. }
            | ValueKind::CallResult { .. }
            | ValueKind::JoinResult { .. } => {
                unreachable!("constants, parameters and authored results emit separately")
            }
        };
        instruction.encode(&mut self.bytes);
    }
}
