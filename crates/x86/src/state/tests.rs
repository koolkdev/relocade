mod synchronization;

use super::{declare, Gpr32, Register, State};
use wasm86_compiler::{Program, Signature, Type, I1, I32};
use wasmparser::{Operator, Parser, Payload};

#[test]
fn named_writes_coalesce_without_crossing_indexed_writes() {
    let mut program = Program::new();
    let memory = declare(&mut program);
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        result: Type::I32,
    });
    let mut body = program.define(function).unwrap();
    let index = body.parameter::<I32>(0).unwrap();
    let mut state = State::new(memory);
    state.write_register(&mut body, Gpr32::Eax, 0).unwrap();
    state.write_register(&mut body, Gpr32::Eax, 1).unwrap();
    state
        .write_register(&mut body, Register::<I32>::indexed(index), 2)
        .unwrap();
    state.write_register(&mut body, Gpr32::Eax, 3).unwrap();
    state.write_register(&mut body, Gpr32::Eax, 4).unwrap();
    state.publish(&mut body, 7, 1).unwrap();
    body.return_(0).unwrap();
    let bytes = program.compile().unwrap();
    let mut stored = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(code) = payload.unwrap() {
            let mut previous_constant = None;
            for operation in code.get_operators_reader().unwrap() {
                let operation = operation.unwrap();
                if matches!(operation, Operator::I32Store { memarg } if memarg.offset == 24) {
                    stored.push(previous_constant.expect("register store has its literal value"));
                }
                previous_constant = match operation {
                    Operator::I32Const { value } => Some(value),
                    _ => None,
                };
            }
        }
    }
    // Index zero names EAX too: the last write must remain after the indexed one.
    assert_eq!(stored, [1, 2, 4]);
}

#[test]
fn publishing_an_exit_keeps_pending_writes_for_the_continuation() {
    let mut program = Program::new();
    let memory = declare(&mut program);
    let function = program.declare(Signature {
        parameters: vec![Type::I1],
        result: Type::I32,
    });
    let mut body = program.define(function).unwrap();
    let stop = body.parameter::<I1>(0).unwrap();
    let mut state = State::new(memory);
    state.write_register(&mut body, Gpr32::Eax, 42).unwrap();
    body.if_(stop, |mut branch| {
        state.publish(&mut branch, 0x1005, 1)?;
        branch.return_(-1)
    })
    .unwrap();
    state.write_register(&mut body, Gpr32::Eax, 43).unwrap();
    state.write_register(&mut body, Gpr32::Ecx, 99).unwrap();
    state.publish(&mut body, 0x100a, 2).unwrap();
    body.return_(0).unwrap();
    program.export("run", function).unwrap();

    let bytes = program.compile().unwrap();
    wasmparser::Validator::new().validate_all(&bytes).unwrap();
    let mut depth = 0;
    let mut exit_stores = Vec::new();
    let mut continuation_stores = Vec::new();
    let mut eax_values = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(code) = payload.unwrap() {
            let mut previous_constant = None;
            for operation in code.get_operators_reader().unwrap() {
                let operation = operation.unwrap();
                if matches!(operation, Operator::I32Store { memarg } if memarg.offset == 24) {
                    eax_values.push((depth, previous_constant.expect("EAX has a literal value")));
                }
                previous_constant = match operation {
                    Operator::I32Const { value } => Some(value),
                    _ => None,
                };
                match operation {
                    Operator::If { .. } => depth += 1,
                    Operator::End if depth > 0 => depth -= 1,
                    Operator::I32Store { memarg } if depth > 0 => {
                        exit_stores.push(memarg.offset);
                    }
                    Operator::I32Store { memarg } => continuation_stores.push(memarg.offset),
                    _ => {}
                }
            }
        }
    }
    assert_eq!(exit_stores, [24, 56, 144]);
    assert_eq!(continuation_stores, [24, 28, 56, 144]);
    assert_eq!(eax_values, [(1, 42), (0, 43)]);
}
