use crate::state::access::cpu_load;
use crate::test_step::{Argument, Event, Input, Observation, Outcome, Snapshot};
use crate::{CpuState, Gpr32};

mod conditional;
mod conditions;
mod fixture;
mod partial;
mod subsets;

use super::super::{Cpu, Register, State};
use crate::flags::{ArithmeticKind, Condition, FlagSource, StatusFlag};
use wasm86_compiler::{BuildError, Program, Signature, Type, I1, I32, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[test]
fn a_terminating_publication_retains_the_earlier_flag_recipe() {
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
                let first = FlagSource::arithmetic(
                    ArithmeticKind::Add,
                    body.value::<I8>(254)?,
                    body.value::<I8>(2)?,
                );
                state.set_flags(&mut body, first)?;
                let stop = body.parameter::<I1>(0)?;
                body.if_(stop, |mut arm| {
                    state.publish(&mut arm, 0x1002, 1)?;
                    arm.return_(1)
                })?;
                let second = FlagSource::arithmetic(
                    ArithmeticKind::Sub,
                    body.value::<I32>(7)?,
                    body.value::<I32>(5)?,
                );
                state.set_flags(&mut body, second)?;
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

fn flag_stores(bytes: &[u8]) -> Vec<(usize, u64, i32)> {
    let mut stores = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut depth = 0;
            let mut literal = None;
            for operation in body.get_operators_reader().unwrap() {
                let operation = operation.unwrap();
                match operation {
                    Operator::If { .. } | Operator::Block { .. } | Operator::Loop { .. } => {
                        depth += 1;
                    }
                    Operator::End if depth > 0 => depth -= 1,
                    Operator::I32Store { memarg } | Operator::I32Store8 { memarg }
                        if memarg.offset <= 17 =>
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
                results: vec![Type::I1],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let arithmetic = FlagSource::arithmetic(
                    ArithmeticKind::Sub,
                    body.value::<I32>(7)?,
                    body.value::<I32>(5)?,
                );
                state.set_flags(&mut body, arithmetic)?;
                let first_stop = body.parameter::<I1>(0)?;
                body.if_(first_stop, |mut arm| {
                    state.publish(&mut arm, 0x1002, 1)?;
                    arm.return_(false)
                })?;
                let result = body.value::<I8>(255)?.add(2);
                state.set_flags(&mut body, FlagSource::Logic { result })?;
                let nonzero = state.condition(&mut body, Condition::NE)?;
                let second_stop = body.parameter::<I1>(1)?;
                body.if_(second_stop, |mut arm| {
                    state.publish(&mut arm, 0x1004, 2)?;
                    arm.return_(nonzero)
                })?;
                let last = FlagSource::arithmetic(
                    ArithmeticKind::Add,
                    body.value::<I32>(11)?,
                    body.value::<I32>(13)?,
                );
                state.set_flags(&mut body, last)?;
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
fn carry_sources_publish_all_concrete_flags_before_the_kind() {
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
                let carry_in = body.value::<I1>(true)?;
                let addition = FlagSource::arithmetic_with_carry(
                    ArithmeticKind::Add,
                    body.value::<I8>(0)?,
                    body.value::<I8>(255)?,
                    carry_in.clone(),
                );
                state.set_flags(&mut body, addition)?;
                let stop = body.parameter::<I1>(0)?;
                body.if_(stop, |mut arm| {
                    state.publish(&mut arm, 0x1002, 1)?;
                    arm.return_(1)
                })?;
                let subtraction = FlagSource::arithmetic_with_carry(
                    ArithmeticKind::Sub,
                    body.value::<I8>(0)?,
                    body.value::<I8>(0)?,
                    carry_in,
                );
                state.set_flags(&mut body, subtraction)?;
                state.publish(&mut body, 0x1004, 2)?;
                body.return_(0)
            },
        )
        .unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    assert_eq!(
        flag_stores(&bytes),
        [
            (1, 12, 1),
            (1, 13, 1),
            (1, 14, 1),
            (1, 15, 1),
            (1, 16, 0),
            (1, 17, 0),
            (1, 0, 0),
            (0, 12, 1),
            (0, 13, 1),
            (0, 14, 1),
            (0, 15, 0),
            (0, 16, 1),
            (0, 17, 0),
            (0, 0, 0),
        ]
    );
}

#[test]
fn invalid_flag_sources_leave_the_previous_source_unchanged() {
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
                parameters: vec![],
                results: vec![Type::I32],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let current = FlagSource::arithmetic(
                    ArithmeticKind::Add,
                    body.value::<I32>(7)?,
                    body.value::<I32>(5)?,
                );
                state.set_flags(&mut body, current)?;
                let invalid = FlagSource::arithmetic(
                    ArithmeticKind::Add,
                    body.value::<I32>(9)?,
                    foreign.clone(),
                );
                assert_eq!(
                    state.set_flags(&mut body, invalid),
                    Err(BuildError::ForeignBody)
                );
                assert_eq!(
                    state.set_flags(
                        &mut body,
                        FlagSource::Logic {
                            result: foreign.clone()
                        }
                    ),
                    Err(BuildError::ForeignBody)
                );
                let invalid_carry = FlagSource::arithmetic_with_carry(
                    ArithmeticKind::Add,
                    body.value::<I32>(9)?,
                    body.value::<I32>(3)?,
                    foreign.eq(0),
                );
                assert_eq!(
                    state.set_flags(&mut body, invalid_carry),
                    Err(BuildError::ForeignBody)
                );
                let mut child_value = None;
                body.if_(false, |mut arm| {
                    child_value = Some(cpu_load!(&mut arm, cpu.memory(), registers.eax)?);
                    Ok(())
                })?;
                let child_value = child_value.unwrap();
                assert_eq!(
                    state.set_flags(
                        &mut body,
                        FlagSource::Logic {
                            result: child_value.clone()
                        }
                    ),
                    Err(BuildError::OutOfScope)
                );
                let invalid_carry = FlagSource::arithmetic_with_carry(
                    ArithmeticKind::Sub,
                    body.value::<I32>(9)?,
                    body.value::<I32>(3)?,
                    child_value.eq(0),
                );
                assert_eq!(
                    state.set_flags(&mut body, invalid_carry),
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

#[test]
fn invalid_explicit_values_leave_the_previous_source_unchanged() {
    let mut foreign_program = Program::new();
    let foreign_function = foreign_program.declare(Signature {
        parameters: vec![Type::I32, Type::I1],
        results: vec![Type::I32],
    });
    let foreign_body = foreign_program.define(foreign_function).unwrap();
    let foreign_result = foreign_body.parameter::<I32>(0).unwrap();
    let foreign_flag = foreign_body.parameter::<I1>(1).unwrap();
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I32],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let current = FlagSource::arithmetic(
                    ArithmeticKind::Add,
                    body.value::<I32>(7)?,
                    body.value::<I32>(5)?,
                );
                state.set_flags(&mut body, current)?;
                let mut child_values = None;
                body.if_(false, |mut arm| {
                    child_values = Some((
                        cpu_load!(&mut arm, cpu.memory(), registers.eax)?,
                        cpu_load!(&mut arm, cpu.memory(), flags.status.cf)?.ne(0),
                    ));
                    Ok(())
                })?;
                let (child_result, child_flag) = child_values.unwrap();
                let valid_result = body.value::<I32>(99)?;
                let valid_flag = body.value::<I1>(false)?;
                for (origin, invalid_result, invalid_flag, error) in [
                    (
                        "foreign",
                        foreign_result,
                        foreign_flag,
                        BuildError::ForeignBody,
                    ),
                    ("child", child_result, child_flag, BuildError::OutOfScope),
                ] {
                    for (field, status_flag) in [
                        ("result", None),
                        ("CF", Some(StatusFlag::CF)),
                        ("PF", Some(StatusFlag::PF)),
                        ("AF", Some(StatusFlag::AF)),
                        ("ZF", Some(StatusFlag::ZF)),
                        ("SF", Some(StatusFlag::SF)),
                        ("OF", Some(StatusFlag::OF)),
                    ] {
                        // Only one retained field is invalid, so another field
                        // cannot mask a missing ownership or visibility check.
                        let source = FlagSource::Explicit {
                            result: if status_flag.is_none() {
                                invalid_result.clone()
                            } else {
                                valid_result.clone()
                            },
                            flags: StatusFlag::ALL.map(|flag| {
                                if status_flag == Some(flag) {
                                    invalid_flag.clone()
                                } else {
                                    valid_flag.clone()
                                }
                            }),
                        };
                        assert_eq!(
                            state.set_flags(&mut body, source),
                            Err(error.clone()),
                            "{origin} {field}"
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
    assert_eq!(flag_stores(&bytes), [(0, 4, 7), (0, 8, 5), (0, 0, 10)]);
}

fn flags_after_register_synchronization() -> crate::CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I1],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let old_eax = state.read_register(&mut body, Gpr32::Eax)?;
                let source =
                    FlagSource::arithmetic(ArithmeticKind::Sub, old_eax, body.value::<I32>(1)?);
                state.set_flags(&mut body, source)?;
                let index = body.parameter::<I32>(0)?;
                state.write_register(&mut body, Register::<I32>::indexed(index), 0)?;
                state.write_register(&mut body, Gpr32::Eax, 0xdead_beefu32)?;
                let stop = body.parameter::<I1>(1)?;
                body.if_(stop, |mut arm| {
                    state.publish(&mut arm, 0x1002, 1)?;
                    arm.return_(7)
                })?;
                let zero = body.value::<I32>(0)?;
                state.set_flags(&mut body, FlagSource::Logic { result: zero })?;
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

#[test]
fn flag_publication_preserves_register_snapshots_in_wasmtime() {
    let module = crate::test_step::TestModule::new(&flags_after_register_synchronization());
    let mut initial = CpuState::filled(0xa5);
    initial.registers.eax = 7;
    initial.registers.ecx = 9;
    initial.eip = 0x1000;
    initial.instruction_count = 0xffff_ffff;
    for index in [0, 1] {
        for stop in [0, 1] {
            let mut expected = initial;
            expected.registers.eax = 0xdead_beef;
            if index == 1 {
                expected.registers.ecx = 0;
            }
            if stop == 1 {
                expected.flags.kind = 9;
                expected.flags.left = 7;
                expected.flags.right = 1;
                expected.eip = 0x1002;
                expected.instruction_count = 0;
            } else {
                expected.flags.kind = 11;
                expected.flags.left = 0;
                // The untaken earlier exit never publishes its right payload.
                expected.eip = 0x1004;
                expected.instruction_count = 1;
            }
            let result = if stop == 1 { 7 } else { 0 };
            let input = Input {
                arguments: vec![Argument::I32(index), Argument::I32(stop)],
                ..Input::new(&initial.to_bytes())
            };
            assert_eq!(
                module.observe(&input, 1),
                Observation {
                    events: vec![Event::Return {
                        outcome: Outcome::Returned(vec![Argument::I64(result)]),
                        snapshot: Snapshot {
                            cpu: expected.to_bytes().to_vec(),
                            guest: None,
                        },
                    }],
                    guest_unchanged: true,
                    machine_unchanged: true,
                },
                "index {index}, stop {stop}"
            );
        }
    }
}
