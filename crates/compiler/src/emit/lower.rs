//! Lower an already ordered value plan without consulting the source body.
use wasm_encoder::Instruction;

use super::{evaluation::Evaluation, function::FunctionEncoder, integer, memory};
use crate::Type;

pub(super) fn values(
    code: &mut FunctionEncoder,
    memories: &[Option<u32>],
    functions: &[Option<u32>],
    plan: Vec<Evaluation>,
) {
    for step in plan {
        match step {
            Evaluation::Constant { ty, bits } => code.instruction(match ty {
                Type::I1 | Type::I8 | Type::I16 | Type::I32 => {
                    Instruction::I32Const(bits as u32 as i32)
                }
                Type::I64 => Instruction::I64Const(bits as i64),
            }),
            Evaluation::Parameter(index) => code.instruction(Instruction::LocalGet(index)),
            Evaluation::Local { slot, operation } => code.local(slot, operation),
            Evaluation::Expression {
                result_type,
                expression,
            } => integer::emit(code, result_type, expression),
            Evaluation::Load {
                memory,
                offset,
                bytes,
                result_type,
                signed,
            } => {
                let argument = memory::argument(memories, memory, offset, bytes);
                code.instruction(memory::load(argument, bytes, result_type, signed));
            }
            Evaluation::Call(target) => code.instruction(Instruction::Call(
                functions[target.0].expect("a call target has a function index"),
            )),
            Evaluation::Drop => code.instruction(Instruction::Drop),
        }
    }
}
