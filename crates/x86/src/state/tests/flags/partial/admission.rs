use crate::alu::ArithmeticOp;
use crate::flags::{FlagChange, StatusFlag};
use crate::state::access::cpu_load;
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::CompiledModule;
use crate::FlagBytes;
use wasm86_compiler::{BuildError, Program, Signature, Type, I1, I32};

use super::super::fixture::{assert_result, initial_cpu};

#[test]
fn rejected_partial_values_and_predicates_leave_pending_changes_intact() {
    let mut foreign_program = Program::new();
    let foreign_function = foreign_program.declare(Signature {
        parameters: vec![Type::I1],
        results: vec![Type::I1],
    });
    let foreign_body = foreign_program.define(foreign_function).unwrap();
    let foreign = foreign_body.parameter::<I1>(0).unwrap();
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
                state.write_flags(&mut body, ArithmeticOp::Add.apply::<I32>(7, 5).flags)?;
                let pending = body.parameter::<I1>(0)?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(StatusFlag::ZF.into(), true.into())]).when(pending),
                )?;
                let mut child = None;
                body.if_(false, |mut arm| {
                    child = Some(cpu_load!(&mut arm, cpu.memory(), registers.eax)?.ne(0));
                    Ok(())
                })?;
                for (invalid, error) in [
                    (foreign, BuildError::ForeignBody),
                    (child.unwrap(), BuildError::OutOfScope),
                ] {
                    for flag in StatusFlag::ALL {
                        assert_eq!(
                            state.write_flags(
                                &mut body,
                                FlagChange::partial([(flag.into(), invalid.clone())])
                            ),
                            Err(error.clone())
                        );
                        for condition in [false, true] {
                            assert_eq!(
                                state.write_flags(
                                    &mut body,
                                    FlagChange::partial([(flag.into(), invalid.clone())])
                                        .when(condition)
                                ),
                                Err(error.clone())
                            );
                        }
                    }
                    assert_eq!(
                        state.write_flags(
                            &mut body,
                            FlagChange::partial([(StatusFlag::CF.into(), false.into())])
                                .when(invalid.clone())
                        ),
                        Err(error.clone())
                    );
                    assert_eq!(
                        state.write_flags(&mut body, FlagChange::partial([]).when(invalid.clone())),
                        Err(error.clone())
                    );
                    for change in [
                        FlagChange::partial([(StatusFlag::CF.into(), true.into())])
                            .when(invalid.clone())
                            .when(false),
                        FlagChange::partial([(StatusFlag::CF.into(), true.into())])
                            .when(false)
                            .when(invalid.clone()),
                        FlagChange::partial([]).when(invalid.clone()).when(false),
                        FlagChange::partial([]).when(false).when(invalid.clone()),
                        FlagChange::partial([(StatusFlag::CF.into(), true.into())])
                            .when(invalid.clone())
                            .preserving(StatusFlag::CF),
                        FlagChange::partial([(StatusFlag::CF.into(), true.into())])
                            .preserving(StatusFlag::CF)
                            .when(invalid.clone()),
                    ] {
                        assert_eq!(state.write_flags(&mut body, change), Err(error.clone()));
                    }
                }
                state.publish(&mut body, 0x1002, 1)?;
                body.return_(0_u64)
            },
        )
        .unwrap();
    foreign_body.return_(false).unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    let initial = initial_cpu();
    for pending in [0, 1] {
        let mut expected = initial;
        if pending == 0 {
            expected.flags.status_source.kind = 10;
            expected.flags.status_source.left = 7;
            expected.flags.status_source.right = 5;
        } else {
            expected.flags.status_source.kind = 0;
            expected.flags.bytes = FlagBytes {
                cf: 0,
                pf: 1,
                af: 0,
                zf: 1,
                sf: 0,
                of: 0,
                ..expected.flags.bytes
            };
        }
        expected.eip = 0x1002;
        expected.instruction_count = 0;
        assert_result(&module, &initial, &[pending], &expected, 0);
    }
}

#[test]
fn empty_and_constant_false_partial_changes_preserve_the_stored_record() {
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
                state.write_flags(&mut body, FlagChange::partial([]))?;
                let predicate = body.parameter::<I1>(0)?;
                state.write_flags(&mut body, FlagChange::partial([]).when(predicate))?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(StatusFlag::CF.into(), false.into())]).when(false),
                )?;
                state.publish(&mut body, 0x1002, 1)?;
                body.return_(0_u64)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    for kind in [0, 9] {
        let mut initial = initial_cpu();
        initial.flags.status_source.kind = kind;
        let mut expected = initial;
        expected.eip = 0x1002;
        expected.instruction_count = 0;
        for predicate in [0, 1] {
            assert_result(&module, &initial, &[predicate], &expected, 0);
        }
    }
}
