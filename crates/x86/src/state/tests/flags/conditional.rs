use crate::alu::flags::{Condition, FlagSource, StatusFlag};
use crate::alu::ArithmeticOp;
use crate::state::access::cpu_load;
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::{CompiledModule, StatusFlags};
use wasm86_compiler::{BuildError, Program, Signature, Type, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

use super::fixture::{assert_result, initial_cpu};

fn conditional_publication(local_base: bool) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I1; 3],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                if local_base {
                    state.set_flags(&mut body, ArithmeticOp::Subtract.apply::<I32>(4, 5).flags)?;
                }
                let initial_equal = state.condition(&mut body, Condition::E)?;
                let first = body.parameter::<I1>(0)?;
                state.set_flags_if(
                    &mut body,
                    first,
                    ArithmeticOp::Add.apply::<I8>(255, 1).flags,
                )?;
                let first_equal = state.condition(&mut body, Condition::E)?;
                let stop = body.parameter::<I1>(2)?;
                body.if_(stop, |mut arm| {
                    state.publish(&mut arm, 0x1002, 1)?;
                    arm.return_(
                        initial_equal
                            .unsigned()
                            .extend::<I64>()
                            .shl(1)
                            .or(first_equal.unsigned().extend::<I64>()),
                    )
                })?;
                let second = body.parameter::<I1>(1)?;
                state.set_flags_if(
                    &mut body,
                    second,
                    FlagSource::<I16>::Logic {
                        result: 0x8000.into(),
                    },
                )?;
                let equal = state.condition(&mut body, Condition::E)?;
                let not_equal = state.condition(&mut body, Condition::NE)?;
                let less = state.condition(&mut body, Condition::L)?;
                let carry = state.condition(&mut body, Condition::B)?;
                state.publish(&mut body, 0x1004, 2)?;
                body.return_(
                    equal
                        .unsigned()
                        .extend::<I64>()
                        .or(not_equal.unsigned().extend::<I64>().shl(1))
                        .or(less.unsigned().extend::<I64>().shl(2))
                        .or(carry.unsigned().extend::<I64>().shl(3)),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

#[test]
fn conditional_flags_keep_earlier_exits_and_publish_only_the_last_active_source() {
    let initial = initial_cpu();
    for local_base in [false, true] {
        let module = TestModule::new(&conditional_publication(local_base));
        for first in [0, 1] {
            for second in [0, 1] {
                for stop in [0, 1] {
                    let mut expected = initial;
                    let active_second = second == 1 && stop == 0;
                    let result = if stop == 1 {
                        i64::from(first)
                    } else if active_second {
                        6 // ZF=0, !ZF=1, SF^OF=1, CF=0.
                    } else if first == 1 {
                        9 // ZF=1, !ZF=0, SF^OF=0, CF=1.
                    } else {
                        14 // ZF=0, !ZF=1, SF^OF=1, CF=1.
                    };
                    if active_second {
                        expected.flags.kind = 7;
                        expected.flags.left = 0x8000;
                        // Earlier symbolic sources never write their unused
                        // payloads into backing, even when both predicates hold.
                    } else if first == 1 {
                        expected.flags.kind = 2;
                        expected.flags.left = 255;
                        expected.flags.right = 1;
                    } else if local_base {
                        expected.flags.left = 4;
                        expected.flags.right = 5;
                    }
                    expected.eip = if stop == 1 { 0x1002 } else { 0x1004 };
                    expected.instruction_count = if stop == 1 { 0 } else { 1 };
                    assert_result(&module, &initial, &[first, second, stop], &expected, result);
                }
            }
        }
    }
}

#[test]
fn preserving_or_consuming_carry_uses_the_selected_conditional_source() {
    for preserve_carry in [false, true] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I1],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let mut state = State::new(&cpu);
                    let replace = body.parameter::<I1>(0)?;
                    state.set_flags_if(
                        &mut body,
                        replace,
                        FlagSource::<I8>::Logic { result: 0.into() },
                    )?;
                    let result = if preserve_carry {
                        let addition = ArithmeticOp::Add.apply::<I8>(255, 1);
                        let result = addition.result.unsigned().extend::<I64>();
                        state.set_flags(&mut body, addition.flags.preserving(StatusFlag::CF))?;
                        result
                    } else {
                        let carry = state.condition(&mut body, Condition::B)?;
                        let addition = ArithmeticOp::Add.apply_with_carry::<I8>(255, 0, carry);
                        let result = addition.result.unsigned().extend::<I64>();
                        state.set_flags(&mut body, addition.flags)?;
                        result
                    };
                    state.publish(&mut body, 0x1004, 2)?;
                    body.return_(result)
                },
            )
            .unwrap();
        program.export("run", function).unwrap();
        let module = TestModule::new(&CompiledModule {
            bytes: program.compile().unwrap(),
            entry: "run".into(),
        });
        let initial = initial_cpu();
        for replace in [0, 1] {
            let zero_result = preserve_carry || replace == 0;
            let mut expected = initial;
            expected.flags.kind = 0;
            expected.flags.status = StatusFlags {
                cf: u8::from(replace == 0),
                pf: 1,
                af: u8::from(zero_result),
                zf: u8::from(zero_result),
                sf: u8::from(!zero_result),
                of: 0,
            };
            expected.eip = 0x1004;
            expected.instruction_count = 1;
            assert_result(
                &module,
                &initial,
                &[replace],
                &expected,
                if zero_result { 0 } else { 255 },
            );
        }
    }
}

#[test]
fn constant_predicates_omit_false_updates_and_discard_history_on_true() {
    for (pending_history, replace) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I1],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let mut state = State::new(&cpu);
                    if pending_history {
                        let pending = body.parameter::<I1>(0)?;
                        state.set_flags_if(
                            &mut body,
                            pending,
                            ArithmeticOp::Add.apply::<I16>(2, 3).flags,
                        )?;
                    }
                    let folded = body
                        .value::<I32>(7)?
                        .add(9)
                        .eq(if replace { 16 } else { 17 });
                    state.set_flags_if(
                        &mut body,
                        folded,
                        FlagSource::<I8>::Logic { result: 0.into() },
                    )?;
                    state.publish(&mut body, 0x1002, 1)?;
                    body.return_(0_u64)
                },
            )
            .unwrap();
        program.export("run", function).unwrap();
        let compiled = CompiledModule {
            bytes: program.compile().unwrap(),
            entry: "run".into(),
        };
        let mut decisions = 0;
        for payload in Parser::new(0).parse_all(&compiled.bytes) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                decisions += body
                    .get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .filter(|operation| matches!(operation.as_ref().unwrap(), Operator::If { .. }))
                    .count();
            }
        }
        assert_eq!(decisions, usize::from(pending_history && !replace));
        let module = TestModule::new(&compiled);
        let initial = initial_cpu();
        for pending in [0, 1] {
            let mut expected = initial;
            if replace {
                expected.flags.kind = 3;
                expected.flags.left = 0;
            } else if pending_history && pending == 1 {
                expected.flags.kind = 6;
                expected.flags.left = 2;
                expected.flags.right = 3;
            }
            expected.eip = 0x1002;
            expected.instruction_count = 0;
            assert_result(&module, &initial, &[pending], &expected, 0);
        }
    }
}

#[test]
fn rejected_conditional_sources_and_predicates_leave_pending_flags_unchanged() {
    let mut foreign_program = Program::new();
    let foreign_function = foreign_program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
    });
    let foreign_body = foreign_program.define(foreign_function).unwrap();
    let foreign = foreign_body.parameter::<I32>(0).unwrap();
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![Type::I1],
                results: vec![Type::I32],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let pending = body.parameter::<I1>(0)?;
                let current = FlagSource::<I8>::Logic { result: 42.into() };
                state.set_flags_if(&mut body, pending, current)?;
                let mut child = None;
                body.if_(false, |mut arm| {
                    child = Some(cpu_load!(&mut arm, cpu.memory(), registers.eax)?);
                    Ok(())
                })?;
                for (invalid, error) in [
                    (foreign, BuildError::ForeignBody),
                    (child.unwrap(), BuildError::OutOfScope),
                ] {
                    let valid = FlagSource::<I32>::Logic { result: 3.into() };
                    assert_eq!(
                        state.set_flags_if(&mut body, invalid.eq(0), valid),
                        Err(error.clone())
                    );
                    for condition in [false, true] {
                        for flag in StatusFlag::ALL {
                            let invalid_source = FlagSource::<I32>::Explicit {
                                flags: StatusFlag::ALL.map(|candidate| {
                                    if candidate == flag {
                                        invalid.eq(0)
                                    } else {
                                        false.into()
                                    }
                                }),
                            };
                            assert_eq!(
                                state.set_flags_if(&mut body, condition, invalid_source),
                                Err(error.clone())
                            );
                        }
                        assert_eq!(
                            state.set_flags_if(
                                &mut body,
                                condition,
                                FlagSource::Logic {
                                    result: invalid.clone()
                                },
                            ),
                            Err(error.clone())
                        );
                    }
                }
                state.publish(&mut body, 0x1002, 1)?;
                body.return_(0)
            },
        )
        .unwrap();
    foreign_body.return_(0).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    assert_eq!(super::flag_stores(&bytes), [(2, 4, 42), (2, 0, 3)]);
}

#[test]
fn conditional_publication_keeps_constant_control_depth_as_history_grows() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I32],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let selector = body.parameter::<I32>(0)?;
                for index in 0..256 {
                    state.set_flags_if(
                        &mut body,
                        selector.eq(index),
                        FlagSource::<I32>::Logic {
                            result: index.into(),
                        },
                    )?;
                }
                state.publish(&mut body, 0x1200, 256)?;
                body.return_(0)
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    let mut depth = 0;
    let mut maximum_depth = 0;
    let mut decisions = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operation in body.get_operators_reader().unwrap() {
                match operation.unwrap() {
                    Operator::Block { .. } | Operator::Loop { .. } => depth += 1,
                    Operator::If { .. } => {
                        depth += 1;
                        decisions += 1;
                    }
                    Operator::End if depth > 0 => depth -= 1,
                    _ => {}
                }
                maximum_depth = maximum_depth.max(depth);
            }
        }
    }
    assert_eq!(decisions, 256);
    assert_eq!(
        maximum_depth, 2,
        "one publication block and its current decision"
    );
}
