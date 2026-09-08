use super::super::{Cpu, State};
use crate::flags::{ArithmeticSource, Condition};
use wasm86_compiler::{Program, Signature, Type, I1, I32, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[test]
fn stored_condition_resolver_is_shared_by_repeated_inverse_queries_and_bodies() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    for name in ["first", "second"] {
        let function = program
            .function(
                Signature {
                    parameters: vec![],
                    result: Type::I32,
                },
                |mut body| {
                    let mut state = State::new(&cpu);
                    let equal = state.condition(&mut body, Condition::E)?;
                    let not_equal = state.condition(&mut body, Condition::NE)?;
                    let repeated = state.condition(&mut body, Condition::E)?;
                    body.return_(
                        equal
                            .unsigned()
                            .extend::<I32>()
                            .shl(1)
                            .or(not_equal.unsigned().extend::<I32>())
                            .or(repeated.unsigned().extend::<I32>().shl(2)),
                    )
                },
            )
            .unwrap();
        program.export(name, function).unwrap();
    }
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut bodies = 0;
    let mut calls = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            bodies += 1;
            for operation in body.get_operators_reader().unwrap() {
                if let Operator::Call { function_index } = operation.unwrap() {
                    calls.push(function_index);
                }
            }
        }
    }
    assert_eq!(bodies, 3, "two consumers need one stored-equality resolver");
    assert_eq!(calls.len(), 2, "each body resolves its input equality once");
    assert_eq!(calls[0], calls[1]);
}

#[test]
fn a_terminating_publication_retains_the_earlier_flag_recipe() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![Type::I1],
                result: Type::I32,
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let first = ArithmeticSource::add(body.value::<I8>(254)?, body.value::<I8>(2)?);
                state.set_arithmetic_flags(&mut body, &first)?;
                let stop = body.parameter::<I1>(0)?;
                body.if_(stop, |mut arm| {
                    state.publish(&mut arm, 0x1002, 1)?;
                    arm.return_(1)
                })?;
                let second =
                    ArithmeticSource::subtract(body.value::<I32>(7)?, body.value::<I32>(5)?);
                state.set_arithmetic_flags(&mut body, &second)?;
                state.publish(&mut body, 0x1007, 2)?;
                body.return_(0)
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut stores = Vec::new();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut depth = 0;
            let mut literal = None;
            for operation in body.get_operators_reader().unwrap() {
                let operation = operation.unwrap();
                match operation {
                    Operator::If { .. } => depth += 1,
                    Operator::End if depth > 0 => depth -= 1,
                    Operator::I32Store { memarg } | Operator::I32Store8 { memarg }
                        if memarg.offset <= 8 =>
                    {
                        stores.push((depth, memarg.offset, literal.unwrap()));
                    }
                    _ => {}
                }
                literal = match operation {
                    Operator::I32Const { value } => Some(value),
                    _ => None,
                };
            }
        }
    }
    assert_eq!(
        stores,
        [
            (1, 4, 254),
            (1, 8, 2),
            (1, 0, 2),
            (0, 4, 7),
            (0, 8, 5),
            (0, 0, 9)
        ]
    );
}

#[test]
fn stored_signed_cmp_conditions_compare_in_the_consumer_and_guard_the_fallback() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![],
                result: Type::I1,
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let less = state.condition(&mut body, Condition::L)?;
                body.return_(less)
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut body_index = 0;
    let mut consumer_signed_compares = 0;
    let mut fallback_call_depths = Vec::new();
    let mut unrelated_work = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut depth = 0;
            for operation in body.get_operators_reader().unwrap() {
                match operation.unwrap() {
                    Operator::If { .. } => depth += 1,
                    Operator::End if depth > 0 => depth -= 1,
                    Operator::I32LtS if body_index == 0 => consumer_signed_compares += 1,
                    Operator::Call { .. } if body_index == 0 => fallback_call_depths.push(depth),
                    Operator::I32Sub
                    | Operator::I32Popcnt
                    | Operator::I32Store { .. }
                    | Operator::I32Store8 { .. }
                    | Operator::I32Store16 { .. }
                    | Operator::I64Store { .. } => unrelated_work += 1,
                    _ => {}
                }
            }
            body_index += 1;
        }
    }
    assert_eq!(
        consumer_signed_compares, 3,
        "the consumer compares every CMP operand width directly"
    );
    assert_eq!(
        fallback_call_depths,
        [1],
        "the consumer calls its fallback only inside the guarded arm"
    );
    assert_eq!(
        unrelated_work, 0,
        "signed condition reads need no subtraction, parity calculation or stores"
    );
}
