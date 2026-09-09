use crate::{
    flags::Condition,
    state::{Cpu, State},
};
use wasm86_compiler::{Program, Signature, Type, I32};
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
                    result: Some(Type::I32),
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
fn stored_signed_cmp_conditions_compare_in_the_consumer_and_guard_the_fallback() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![],
                result: Some(Type::I1),
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
    let mut consumer_switches = 0;
    let mut consumer_selections = 0;
    let mut fallback_calls = 0;
    let mut consumer_cases_started = false;
    let mut unrelated_work = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operation in body.get_operators_reader().unwrap() {
                match operation.unwrap() {
                    Operator::I32LtS if body_index == 0 => consumer_signed_compares += 1,
                    Operator::BrTable { .. } if body_index == 0 => {
                        consumer_switches += 1;
                        consumer_cases_started = true;
                    }
                    Operator::Select if body_index == 0 => consumer_selections += 1,
                    Operator::Call { .. } if body_index == 0 => fallback_calls += 1,
                    // Table-index normalization precedes the selected predicate.
                    Operator::I32Sub if body_index != 0 || consumer_cases_started => {
                        unrelated_work += 1;
                    }
                    Operator::I32Popcnt
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
        (consumer_switches, consumer_selections, fallback_calls),
        (1, 0, 1),
        "one kind switch chooses a typed predicate or the shared fallback"
    );
    assert_eq!(
        unrelated_work, 0,
        "signed predicates need no subtraction, parity calculation or stores"
    );
}

#[test]
fn stored_logical_equality_reads_only_the_result_in_the_consumer() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![],
                result: Some(Type::I1),
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let equal = state.condition(&mut body, Condition::E)?;
                body.return_(equal)
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut left_reads = 0;
    let mut right_reads = 0;
    let mut logical_cases = 0;
    let mut case_left_reads = 0;
    let mut case_right_reads = 0;
    let mut case_zero_tests = 0;
    let mut kind_switches = 0;
    let mut eager_selections = 0;
    let mut fallback_calls = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operation in body.get_operators_reader().unwrap() {
                match operation.unwrap() {
                    Operator::I32Load { memarg } if memarg.offset == 4 => {
                        left_reads += 1;
                        case_left_reads += 1;
                    }
                    Operator::I32Load { memarg } if memarg.offset == 8 => {
                        right_reads += 1;
                        case_right_reads += 1;
                    }
                    Operator::I32Eqz => case_zero_tests += 1,
                    Operator::BrTable { .. } => kind_switches += 1,
                    Operator::Br { .. } if kind_switches == 1 => {
                        // A direct case ends by joining its result. Logic reads A
                        // alone; narrow SUB equality can also use a zero test.
                        if (case_left_reads, case_right_reads) == (1, 0) {
                            assert_eq!(case_zero_tests, 1);
                            logical_cases += 1;
                        }
                        case_left_reads = 0;
                        case_right_reads = 0;
                        case_zero_tests = 0;
                    }
                    Operator::Select => eager_selections += 1,
                    Operator::Call { .. } => fallback_calls += 1,
                    _ => {}
                }
            }
            break; // The first body is the condition consumer, not its resolver.
        }
    }
    // Each width has one SUB case reading A/B and one logic case reading A only.
    assert_eq!((left_reads, right_reads), (6, 3));
    assert_eq!(
        logical_cases, 3,
        "each stored width tests its logical result without reading B"
    );
    assert_eq!((kind_switches, eager_selections, fallback_calls), (1, 0, 1));
}
