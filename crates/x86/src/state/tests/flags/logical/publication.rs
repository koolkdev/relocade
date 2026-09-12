use super::super::fixture::{assert_result, initial_cpu};
use crate::alu::ArithmeticOp;
use crate::flags::{Condition, Flag, FlagChange};
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::CompiledModule;
use crate::FlagBytes;
use wasm86_compiler::{Program, Signature, Type, I1, I32, I64, I8};

#[test]
fn exit_publication_freezes_both_flag_histories_without_poisoning_later_queries() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I1, Type::I1, Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let direction_predicate = body.parameter::<I1>(0)?;
                let carry_value = body.parameter::<I1>(1)?;
                let stop = body.parameter::<I32>(2)?;
                let incoming_carry = state.read_flag(&mut body, Flag::CF)?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(Flag::DF, true.into())]).when(direction_predicate),
                )?;
                let direction = state.read_flag(&mut body, Flag::DF)?;
                body.if_(stop.eq(1), |mut arm| {
                    state.publish(&mut arm, 0x1001, 1)?;
                    arm.return_(
                        incoming_carry
                            .unsigned()
                            .extend::<I64>()
                            .or(direction.unsigned().extend::<I64>().shl(1)),
                    )
                })?;
                state.write_flag(&mut body, Flag::CF, carry_value)?;
                let carry = state.read_flag(&mut body, Flag::CF)?;
                body.if_(stop.eq(2), |mut arm| {
                    state.publish(&mut arm, 0x1002, 2)?;
                    arm.return_(
                        carry
                            .unsigned()
                            .extend::<I64>()
                            .or(direction.unsigned().extend::<I64>().shl(1)),
                    )
                })?;
                // The earlier concrete publication resolves AF inside its exit arm.
                // This live-path query must not reuse that descendant-scoped value.
                let auxiliary = state.read_flag(&mut body, Flag::AF)?;
                state.write_flags(&mut body, ArithmeticOp::Add.apply::<I8>(0xff, 1).flags)?;
                state.write_flag(&mut body, Flag::DF, false)?;
                let carry = state.read_flag(&mut body, Flag::CF)?;
                let direction = state.read_flag(&mut body, Flag::DF)?;
                let zero = state.condition(&mut body, Condition::E)?;
                state.publish(&mut body, 0x1004, 4)?;
                body.return_(
                    carry
                        .unsigned()
                        .extend::<I64>()
                        .or(direction.unsigned().extend::<I64>().shl(1))
                        .or(zero.unsigned().extend::<I64>().shl(2))
                        .or(auxiliary.unsigned().extend::<I64>().shl(3)),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    let mut initial = initial_cpu();
    initial.flags.bytes.df = 0xfe;
    for direction in [0, 1] {
        for carry in [0, 1] {
            for stop in [0, 1, 2] {
                let mut expected = initial;
                if direction != 0 {
                    expected.flags.bytes.df = 1;
                }
                let result = match stop {
                    1 => {
                        expected.eip = 0x1001;
                        expected.instruction_count = 0;
                        1 + i64::from(direction) * 2
                    }
                    2 => {
                        expected.flags.status_source.kind = 0;
                        expected.flags.bytes = FlagBytes {
                            cf: carry,
                            pf: 1,
                            af: 1,
                            zf: 0,
                            sf: 1,
                            of: 0,
                            ..expected.flags.bytes
                        };
                        expected.eip = 0x1002;
                        expected.instruction_count = 1;
                        i64::from(carry) + i64::from(direction) * 2
                    }
                    _ => {
                        expected.flags.status_source.kind = 2;
                        expected.flags.status_source.left = 0xff;
                        expected.flags.status_source.right = 1;
                        expected.flags.bytes.df = 0;
                        expected.eip = 0x1004;
                        expected.instruction_count = 3;
                        13
                    }
                };
                assert_result(
                    &module,
                    &initial,
                    &[direction, i32::from(carry), stop],
                    &expected,
                    result,
                );
            }
        }
    }
}
