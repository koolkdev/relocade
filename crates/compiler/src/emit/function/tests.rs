use super::{FunctionEncoder, LocalOp};
use wasm_encoder::{BlockType, Instruction, ValType};
use wasmparser::{BinaryReader, FunctionBody, Operator};

#[test]
fn deferred_accesses_preserve_order_and_use_indices_after_parameters() {
    let mut code = FunctionEncoder::new(130, vec![ValType::I32, ValType::I64, ValType::I32]);
    let temporary = 2;
    code.instruction(Instruction::LocalGet(129));
    code.local(0, LocalOp::Set);
    code.local(0, LocalOp::Get);
    code.local(temporary, LocalOp::Tee);
    code.instruction(Instruction::Return);

    // The unused i64 slot has no declaration. The temporary reuses the i32
    // local after its last get, even though that value remains on the stack.
    assert_eq!(
        code.finish().into_raw_body(),
        [
            0x01, 0x01, 0x7f, // one i32 local
            0x20, 0x81, 0x01, // local.get 129 (parameter)
            0x21, 0x82, 0x01, // local.set 130
            0x20, 0x82, 0x01, // local.get 130
            0x22, 0x82, 0x01, // local.tee 130
            0x0f, 0x0b, // return; end
        ]
    );
}

#[test]
fn nested_loop_backedges_keep_live_values_apart_from_body_temporaries() {
    let mut code = FunctionEncoder::new(0, vec![ValType::I32; 6]);
    code.instruction(Instruction::I32Const(10));
    code.local(0, LocalOp::Set);
    code.instruction(Instruction::Loop(BlockType::Empty));
    code.local(0, LocalOp::Get);
    code.instruction(Instruction::Drop);
    code.instruction(Instruction::I32Const(20));
    code.local(1, LocalOp::Set);
    code.instruction(Instruction::Block(BlockType::Empty));
    code.instruction(Instruction::Loop(BlockType::Empty));
    code.local(1, LocalOp::Get);
    code.instruction(Instruction::Drop);
    code.instruction(Instruction::I32Const(1));
    code.instruction(Instruction::If(BlockType::Empty));
    code.instruction(Instruction::I32Const(30));
    code.local(2, LocalOp::Set);
    code.local(2, LocalOp::Get);
    code.instruction(Instruction::Drop);
    code.instruction(Instruction::Else);
    code.instruction(Instruction::Nop);
    code.instruction(Instruction::End);
    code.instruction(Instruction::I32Const(0));
    code.instruction(Instruction::BrIf(0));
    code.instruction(Instruction::End);
    code.instruction(Instruction::End);
    code.local(0, LocalOp::Get);
    code.instruction(Instruction::Drop);
    code.instruction(Instruction::I32Const(40));
    code.local(3, LocalOp::Set);
    code.local(3, LocalOp::Get);
    code.instruction(Instruction::Drop);
    code.instruction(Instruction::I32Const(0));
    code.instruction(Instruction::BrIf(0));
    code.instruction(Instruction::End);
    code.instruction(Instruction::I32Const(50));
    code.local(4, LocalOp::Set);
    code.local(4, LocalOp::Get);
    code.instruction(Instruction::Drop);
    code.instruction(Instruction::I32Const(60));
    code.local(5, LocalOp::Set);
    code.local(5, LocalOp::Get);
    code.instruction(Instruction::Return);

    let bytes = code.finish().into_raw_body();
    let body = FunctionBody::new(BinaryReader::new(&bytes, 0));
    let locals: Vec<_> = body
        .get_locals_reader()
        .unwrap()
        .into_iter()
        .map(Result::unwrap)
        .collect();
    assert_eq!(locals, [(3, wasmparser::ValType::I32)]);
    let stores: Vec<_> = body
        .get_operators_reader()
        .unwrap()
        .into_iter()
        .filter_map(|operator| match operator.unwrap() {
            Operator::LocalSet { local_index } => Some(local_index),
            _ => None,
        })
        .collect();
    // The inner loop needs all three locals. After its exit, the outer-body
    // temporary can reuse local 1. Local 0 becomes reusable after the outer exit.
    assert_eq!(stores, [0, 1, 2, 1, 1, 0]);
}
