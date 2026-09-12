use super::super::fixture::{assert_result, initial_cpu};
use crate::alu::ArithmeticOp;
use crate::flags::{Flag, FlagChange};
use crate::state::access::cpu_load;
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::CompiledModule;
use wasm86_compiler::{BuildError, Program, Signature, Type, I1, I64, I8};

#[test]
fn rejected_mixed_changes_leave_status_and_direction_history_intact() {
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
                state.write_flags(&mut body, ArithmeticOp::Add.apply::<I8>(0x7f, 1).flags)?;
                let predicate = body.parameter::<I1>(0)?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(Flag::DF, true.into())]).when(predicate),
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
                    for invalid_flag in [Flag::CF, Flag::DF] {
                        assert_eq!(
                            state.write_flag(&mut body, invalid_flag, invalid.clone()),
                            Err(error.clone())
                        );
                        for predicate in [None, Some(false), Some(true)] {
                            let carry = if invalid_flag == Flag::CF {
                                invalid.clone()
                            } else {
                                true.into()
                            };
                            let direction = if invalid_flag == Flag::DF {
                                invalid.clone()
                            } else {
                                false.into()
                            };
                            let change =
                                FlagChange::partial([(Flag::CF, carry), (Flag::DF, direction)]);
                            let change = match predicate {
                                Some(value) => change.when(value),
                                None => change,
                            };
                            assert_eq!(state.write_flags(&mut body, change), Err(error.clone()));
                        }
                    }
                    for change in [
                        FlagChange::partial([(Flag::CF, true.into()), (Flag::DF, false.into())])
                            .when(invalid.clone()),
                        FlagChange::partial([]).when(invalid.clone()),
                        FlagChange::partial([(Flag::DF, false.into())])
                            .when(invalid.clone())
                            .when(false),
                        FlagChange::partial([(Flag::DF, false.into())])
                            .when(false)
                            .when(invalid),
                    ] {
                        assert_eq!(state.write_flags(&mut body, change), Err(error.clone()));
                    }
                }
                let carry = state.read_flag(&mut body, Flag::CF)?;
                let direction = state.read_flag(&mut body, Flag::DF)?;
                let overflow = state.read_flag(&mut body, Flag::OF)?;
                state.publish(&mut body, 0x1002, 2)?;
                body.return_(
                    carry
                        .unsigned()
                        .extend::<I64>()
                        .or(direction.unsigned().extend::<I64>().shl(1))
                        .or(overflow.unsigned().extend::<I64>().shl(2)),
                )
            },
        )
        .unwrap();
    foreign_body.return_(false).unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    let mut initial = initial_cpu();
    initial.flags.bytes.df = 0xfe;
    for predicate in [0, 1] {
        let mut expected = initial;
        expected.flags.status_source.kind = 2;
        expected.flags.status_source.left = 0x7f;
        expected.flags.status_source.right = 1;
        if predicate != 0 {
            expected.flags.bytes.df = 1;
        }
        expected.eip = 0x1002;
        expected.instruction_count = 1;
        assert_result(
            &module,
            &initial,
            &[predicate],
            &expected,
            4 + i64::from(predicate) * 2,
        );
    }
}
