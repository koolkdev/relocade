use super::super::{Cpu, Gpr32, Register, State};
use crate::flags::{ArithmeticSource, Condition};
use wasm86_compiler::{BuildError, Program, Signature, Type, I1, I32, I8};
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
    let stores = flag_stores(&bytes);
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
                result: Type::I1,
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

fn flag_stores(bytes: &[u8]) -> Vec<(usize, u64, i32)> {
    let mut stores = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
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
    stores
}

#[test]
fn changing_flag_sources_keeps_payloads_before_kind_and_earlier_exits() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![Type::I1; 2],
                result: Type::I1,
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let arithmetic =
                    ArithmeticSource::subtract(body.value::<I32>(7)?, body.value::<I32>(5)?);
                state.set_arithmetic_flags(&mut body, &arithmetic)?;
                let first_stop = body.parameter::<I1>(0)?;
                body.if_(first_stop, |mut arm| {
                    state.publish(&mut arm, 0x1002, 1)?;
                    arm.return_(false)
                })?;
                let result = body.value::<I8>(255)?.add(2);
                state.set_logic_flags(&mut body, &result)?;
                let nonzero = state.condition(&mut body, Condition::NE)?;
                let second_stop = body.parameter::<I1>(1)?;
                body.if_(second_stop, |mut arm| {
                    state.publish(&mut arm, 0x1004, 2)?;
                    arm.return_(nonzero)
                })?;
                let last = ArithmeticSource::add(body.value::<I32>(11)?, body.value::<I32>(13)?);
                state.set_arithmetic_flags(&mut body, &last)?;
                state.publish(&mut body, 0x1006, 3)?;
                body.return_(true)
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    assert_eq!(
        flag_stores(&bytes),
        [
            (1, 4, 7),
            (1, 8, 5),
            (1, 0, 9),
            (1, 4, 1),
            (1, 0, 3),
            (0, 4, 11),
            (0, 8, 13),
            (0, 0, 10),
        ]
    );
}

#[test]
fn invalid_flag_sources_leave_the_previous_source_unchanged() {
    let mut foreign_program = Program::new();
    let foreign_function = foreign_program.declare(Signature {
        parameters: vec![Type::I32],
        result: Type::I32,
    });
    let foreign_body = foreign_program.define(foreign_function).unwrap();
    let foreign = foreign_body.parameter::<I32>(0).unwrap();
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![],
                result: Type::I32,
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let current = ArithmeticSource::add(body.value::<I32>(7)?, body.value::<I32>(5)?);
                state.set_arithmetic_flags(&mut body, &current)?;
                let invalid = ArithmeticSource::add(body.value::<I32>(9)?, foreign.clone());
                assert_eq!(
                    state.set_arithmetic_flags(&mut body, &invalid),
                    Err(BuildError::ForeignBody)
                );
                assert_eq!(
                    state.set_logic_flags(&mut body, &foreign),
                    Err(BuildError::ForeignBody)
                );
                let mut child_value = None;
                body.if_(false, |mut arm| {
                    child_value = Some(arm.load::<I32>(cpu.memory(), 24)?);
                    Ok(())
                })?;
                assert_eq!(
                    state.set_logic_flags(&mut body, &child_value.unwrap()),
                    Err(BuildError::OutOfScope)
                );
                state.publish(&mut body, 0x1002, 1)?;
                body.return_(0)
            },
        )
        .unwrap();
    foreign_body.return_(0).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    assert_eq!(flag_stores(&bytes), [(0, 4, 7), (0, 8, 5), (0, 0, 10)]);
}

fn flags_after_register_synchronization() -> crate::CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I1],
                result: Type::I64,
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let old_eax = state.read_register(&mut body, Gpr32::Eax)?;
                let source = ArithmeticSource::subtract(old_eax, body.value::<I32>(1)?);
                state.set_arithmetic_flags(&mut body, &source)?;
                let index = body.parameter::<I32>(0)?;
                state.write_register(&mut body, Register::<I32>::indexed(index), 0)?;
                state.write_register(&mut body, Gpr32::Eax, 0xdead_beefu32)?;
                let stop = body.parameter::<I1>(1)?;
                body.if_(stop, |mut arm| {
                    state.publish(&mut arm, 0x1002, 1)?;
                    arm.return_(7)
                })?;
                let zero = body.value::<I32>(0)?;
                state.set_logic_flags(&mut body, &zero)?;
                state.publish(&mut body, 0x1004, 2)?;
                body.return_(0)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    crate::CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_flag_publication(flags: &[&str]) {
    use std::fmt::Write as _;
    let module = crate::test_step::ModuleFile::new(&flags_after_register_synchronization());
    let mut initial = [0xa5u8; 152];
    for (offset, value) in [(24, 7u32), (28, 9), (56, 0x1000), (144, 0xffff_ffff)] {
        initial[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for index in [0, 1] {
        for stop in [0, 1] {
            let mut expected = initial;
            expected[0] = if stop == 1 { 9 } else { 11 };
            let patches = if stop == 1 {
                vec![(4, 7u32), (8, 1), (24, 0xdead_beef), (56, 0x1002), (144, 0)]
            } else {
                // The untaken earlier exit never publishes its right payload.
                vec![(4, 0u32), (24, 0xdead_beef), (56, 0x1004), (144, 1)]
            };
            if index == 1 {
                expected[28..32].copy_from_slice(&0u32.to_le_bytes());
            }
            for (offset, value) in patches {
                expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            let result = if stop == 1 { 7 } else { 0 };
            let mut observation = format!("return {result}\nstate ");
            for byte in expected {
                write!(&mut observation, "{byte:02x}").unwrap();
            }
            observation.push_str("\nguest unchanged\nmachine unchanged\n");
            let input = format!("[{initial:?},[],[],[[\"i32\",{index}],[\"i32\",{stop}]]]");
            assert_eq!(module.observe(flags, &input, 1), observation);
        }
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn flag_publication_preserves_register_snapshots_in_v8() {
    check_flag_publication(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn flag_publication_preserves_register_snapshots_in_v8_optimizing() {
    check_flag_publication(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
