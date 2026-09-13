use super::super::fixture::{assert_result, initial_cpu};
use crate::alu::ArithmeticOp;
use crate::flags::{Condition, Flag, FlagChange, StatusFlag};
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::CompiledModule;
use crate::FlagBytes;
use wasm86_compiler::{Program, Signature, Type, I1, I64, I8};

#[test]
fn conditional_direction_writes_preserve_the_original_byte_when_inactive() {
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
                let first = body.parameter::<I1>(0)?;
                let second = body.parameter::<I1>(1)?;
                let direction = body.parameter::<I1>(2)?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(Flag::DF, false.into())]).when(false),
                )?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(Flag::DF, direction)])
                        .when(true)
                        .when(first)
                        .when(second),
                )?;
                let direction = state.read_flag(&mut body, Flag::DF)?;
                let carry = state.read_flag(&mut body, Flag::CF)?;
                state.publish(&mut body, 0x1001, 1)?;
                body.return_(
                    direction
                        .unsigned()
                        .extend::<I64>()
                        .or(carry.unsigned().extend::<I64>().shl(1)),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    for kind in [0, 9] {
        for stored_direction in [0xfe, 0xff] {
            let mut initial = initial_cpu();
            initial.flags.status_source.kind = kind;
            initial.flags.bytes.df = stored_direction;
            for (first, second, active) in
                [(0, 0, false), (0, 1, false), (1, 0, false), (1, 1, true)]
            {
                for direction in [0, 1] {
                    let mut expected = initial;
                    if active {
                        expected.flags.bytes.df = direction;
                    }
                    expected.eip = 0x1001;
                    expected.instruction_count = 0;
                    let result = 2 + i64::from(expected.flags.bytes.df & 1);
                    assert_result(
                        &module,
                        &initial,
                        &[first, second, i32::from(direction)],
                        &expected,
                        result,
                    );
                }
            }
        }
    }
}

#[test]
fn independent_conditional_carry_and_direction_changes_preserve_each_other() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I1; 2],
                results: vec![Type::I64],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let carry = body.parameter::<I1>(0)?;
                let direction = body.parameter::<I1>(1)?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(Flag::CF, false.into())]).when(carry),
                )?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(Flag::DF, true.into())]).when(direction),
                )?;
                let carry = state.read_flag(&mut body, Flag::CF)?;
                let direction = state.read_flag(&mut body, Flag::DF)?;
                state.publish(&mut body, 0x1002, 2)?;
                body.return_(
                    carry
                        .unsigned()
                        .extend::<I64>()
                        .or(direction.unsigned().extend::<I64>().shl(1)),
                )
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    });
    let mut initial = initial_cpu();
    initial.flags.bytes.df = 0xfe;
    for (carry, direction, result) in [(0, 0, 1), (0, 1, 3), (1, 0, 0), (1, 1, 2)] {
        let mut expected = initial;
        if carry != 0 {
            expected.flags.status_source.kind = 0;
            expected.flags.bytes = FlagBytes {
                cf: 0,
                pf: 1,
                af: 1,
                zf: 0,
                sf: 1,
                of: 0,
                ..expected.flags.bytes
            };
        }
        if direction != 0 {
            expected.flags.bytes.df = 1;
        }
        expected.eip = 0x1002;
        expected.instruction_count = 1;
        assert_result(&module, &initial, &[carry, direction], &expected, result);
    }
}

#[test]
fn full_and_masked_arithmetic_sources_preserve_conditional_direction() {
    for preserve_carry in [false, true] {
        for direction_first in [false, true] {
            let mut program = Program::new();
            let cpu = Cpu::declare(&mut program);
            let function = program
                .function(
                    Signature {
                        parameters: vec![Type::I1; 2],
                        results: vec![Type::I64],
                    },
                    |mut body| {
                        let mut state = State::new(&cpu);
                        let direction = body.parameter::<I1>(0)?;
                        let arithmetic = body.parameter::<I1>(1)?;
                        if direction_first {
                            state.write_flags(
                                &mut body,
                                FlagChange::partial([(Flag::DF, true.into())])
                                    .when(direction.clone()),
                            )?;
                        }
                        let flags = ArithmeticOp::Add.apply::<I8>(0x7f, 1).flags;
                        let flags = if preserve_carry {
                            flags.preserving(StatusFlag::CF)
                        } else {
                            flags
                        };
                        state.write_flags(&mut body, flags.when(arithmetic))?;
                        if !direction_first {
                            state.write_flags(
                                &mut body,
                                FlagChange::partial([(Flag::DF, true.into())]).when(direction),
                            )?;
                        }
                        let carry = state.read_flag(&mut body, Flag::CF)?;
                        let direction = state.read_flag(&mut body, Flag::DF)?;
                        let overflow = state.read_flag(&mut body, Flag::OF)?;
                        let less = state.condition(&mut body, Condition::L)?;
                        state.publish(&mut body, 0x1003, 3)?;
                        body.return_(
                            carry
                                .unsigned()
                                .extend::<I64>()
                                .or(direction.unsigned().extend::<I64>().shl(1))
                                .or(overflow.unsigned().extend::<I64>().shl(2))
                                .or(less.unsigned().extend::<I64>().shl(3)),
                        )
                    },
                )
                .unwrap();
            program.export("run", function).unwrap();
            let module = TestModule::new(&CompiledModule {
                segment_profile: None,
                bytes: program.compile().unwrap(),
                entry: "run".into(),
            });
            let mut initial = initial_cpu();
            initial.flags.bytes.df = 0xfe;
            for direction in [0, 1] {
                for arithmetic in [0, 1] {
                    let mut expected = initial;
                    if direction != 0 {
                        expected.flags.bytes.df = 1;
                    }
                    if arithmetic != 0 {
                        if preserve_carry {
                            expected.flags.status_source.kind = 0;
                            expected.flags.bytes = FlagBytes {
                                cf: 1,
                                pf: 0,
                                af: 1,
                                zf: 0,
                                sf: 1,
                                of: 1,
                                ..expected.flags.bytes
                            };
                        } else {
                            expected.flags.status_source.kind = 2;
                            expected.flags.status_source.left = 0x7f;
                            expected.flags.status_source.right = 1;
                        }
                    }
                    expected.eip = 0x1003;
                    expected.instruction_count = 2;
                    let status = if arithmetic == 0 {
                        9
                    } else if preserve_carry {
                        5
                    } else {
                        4
                    };
                    assert_result(
                        &module,
                        &initial,
                        &[direction, arithmetic],
                        &expected,
                        status + i64::from(direction) * 2,
                    );
                }
            }
        }
    }
}
