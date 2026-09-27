//! Lower a completed function schedule and encode its WebAssembly body.
use std::borrow::Cow;

use wasm_encoder::{Function, Instruction as Wasm, ValType};

use crate::{
    schedule::{Instruction, Schedule},
    Type,
};

mod function;
mod integer;
mod memory;

use function::FunctionEncoder;

pub(super) fn wasm_type(ty: Type) -> ValType {
    match ty {
        Type::I1 | Type::I8 | Type::I16 | Type::I32 => ValType::I32,
        Type::I64 => ValType::I64,
    }
}

pub(super) fn encode(
    schedule: Schedule,
    parameter_count: u32,
    memories: &[Option<u32>],
    functions: &[Option<u32>],
) -> Function {
    let mut code = FunctionEncoder::new(parameter_count, schedule.local_types);
    for instruction in schedule.instructions {
        match instruction {
            Instruction::Constant { ty, bits } => code.instruction(match ty {
                Type::I1 | Type::I8 | Type::I16 | Type::I32 => Wasm::I32Const(bits as u32 as i32),
                Type::I64 => Wasm::I64Const(bits as i64),
            }),
            Instruction::Parameter(index) => code.instruction(Wasm::LocalGet(index)),
            Instruction::Local { slot, operation } => code.local(slot, operation),
            Instruction::Expression {
                result_type,
                expression,
            } => {
                integer::emit(&mut code, result_type, expression);
            }
            Instruction::Load {
                location,
                result_type,
                signed,
            } => {
                let argument = memory::argument(memories, location);
                code.instruction(memory::load(argument, location.bytes, result_type, signed));
            }
            Instruction::Store(location) => {
                let argument = memory::argument(memories, location);
                code.instruction(memory::store(argument, location.bytes));
            }
            Instruction::Atomic(access) => {
                let argument = memory::argument(memories, access.location);
                code.instruction(memory::atomic(
                    argument,
                    access.location.bytes,
                    access.operation,
                ));
            }
            Instruction::Fence => code.instruction(Wasm::AtomicFence),
            Instruction::Call(target) => code.instruction(Wasm::Call(
                functions[target.0].expect("a call target has a function index"),
            )),
            Instruction::TailCall(target) => code.instruction(Wasm::ReturnCall(
                functions[target.0].expect("a tail-call target has a function index"),
            )),
            Instruction::Block(ty) => code.instruction(Wasm::Block(ty)),
            Instruction::Loop(ty) => code.instruction(Wasm::Loop(ty)),
            Instruction::If(ty) => code.instruction(Wasm::If(ty)),
            Instruction::Else => code.instruction(Wasm::Else),
            Instruction::End => code.instruction(Wasm::End),
            Instruction::Branch(depth) => code.instruction(Wasm::Br(depth)),
            Instruction::BranchIf(depth) => code.instruction(Wasm::BrIf(depth)),
            Instruction::BranchTable { targets, default } => {
                code.instruction(Wasm::BrTable(Cow::Owned(targets), default));
            }
            Instruction::Drop => code.instruction(Wasm::Drop),
            Instruction::Return => code.instruction(Wasm::Return),
            Instruction::Trap => code.instruction(Wasm::Unreachable),
        }
    }
    code.finish()
}
