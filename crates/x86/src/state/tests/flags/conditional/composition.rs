use crate::alu::ArithmeticOp;
use crate::flags::{Condition, FlagChange, StatusFlag};
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::CompiledModule;
use crate::FlagBytes;
use wasm86_compiler::{Program, Signature, Type, I1, I32, I64, I8};

use super::super::fixture::{assert_result, initial_cpu};

#[test]
fn every_predicate_must_hold_for_complete_and_partial_changes() {
    for partial in [false, true] {
        for pending_base in [false, true] {
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
                        if pending_base {
                            state.write_flags(
                                &mut body,
                                ArithmeticOp::Subtract.apply::<I32>(4, 5).flags,
                            )?;
                        }
                        let first = body.parameter::<I1>(0)?;
                        let second = body.parameter::<I1>(1)?;
                        let change = if partial {
                            FlagChange::partial([
                                (StatusFlag::CF.into(), false.into()),
                                (StatusFlag::OF.into(), true.into()),
                            ])
                        } else {
                            ArithmeticOp::Add.apply::<I8>(127, 1).flags
                        };
                        state.write_flags(&mut body, change.when(first).when(second))?;
                        let carry = state.condition(&mut body, Condition::B)?;
                        let overflow = state.condition(&mut body, Condition::O)?;
                        state.publish(&mut body, 0x1002, 1)?;
                        body.return_(
                            carry
                                .unsigned()
                                .extend::<I64>()
                                .shl(1)
                                .or(overflow.unsigned().extend::<I64>()),
                        )
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
                for (first, second, active) in
                    [(0, 0, false), (0, 1, false), (1, 0, false), (1, 1, true)]
                {
                    let mut expected = initial;
                    if active && partial {
                        expected.flags.status_source.kind = 0;
                        expected.flags.bytes = FlagBytes {
                            cf: 0,
                            pf: 1,
                            af: 1,
                            zf: u8::from(kind == 0 && !pending_base),
                            sf: 1,
                            of: 1,
                            ..expected.flags.bytes
                        };
                    } else if active {
                        expected.flags.status_source.kind = 2;
                        expected.flags.status_source.left = 127;
                        expected.flags.status_source.right = 1;
                    } else if pending_base {
                        expected.flags.status_source.kind = 9;
                        expected.flags.status_source.left = 4;
                        expected.flags.status_source.right = 5;
                    }
                    expected.eip = 0x1002;
                    expected.instruction_count = 0;
                    let result = if active {
                        1
                    } else if pending_base || kind == 9 {
                        2
                    } else {
                        3
                    };
                    assert_result(&module, &initial, &[first, second], &expected, result);
                }
            }
        }
    }
}

#[test]
fn preserving_carry_keeps_the_predicate_in_either_order() {
    for partial in [false, true] {
        for condition_first in [false, true] {
            for pending_base in [false, true] {
                for carry in [0, 1] {
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
                                if pending_base {
                                    // 127+1 and 128+128 both overflow, with opposite CF.
                                    let (left, right) =
                                        if carry == 0 { (127, 1) } else { (128, 128) };
                                    state.write_flags(
                                        &mut body,
                                        ArithmeticOp::Add.apply::<I8>(left, right).flags,
                                    )?;
                                }
                                let predicate = body.parameter::<I1>(0)?;
                                let change = if partial {
                                    FlagChange::partial([
                                        (StatusFlag::CF.into(), false.into()),
                                        (StatusFlag::OF.into(), false.into()),
                                    ])
                                } else {
                                    ArithmeticOp::Add.apply::<I8>(255, 1).flags
                                };
                                let change = if condition_first {
                                    change.when(predicate).preserving(StatusFlag::CF)
                                } else {
                                    change.preserving(StatusFlag::CF).when(predicate)
                                };
                                state.write_flags(&mut body, change)?;
                                let carry = state.condition(&mut body, Condition::B)?;
                                let overflow = state.condition(&mut body, Condition::O)?;
                                state.publish(&mut body, 0x1002, 1)?;
                                body.return_(
                                    carry
                                        .unsigned()
                                        .extend::<I64>()
                                        .shl(1)
                                        .or(overflow.unsigned().extend::<I64>()),
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
                    initial.flags.status_source.kind = 0;
                    initial.flags.bytes = FlagBytes {
                        cf: 0xfe | if pending_base { 1 - carry } else { carry },
                        pf: 0x7f,
                        af: 0x80,
                        zf: 0xff,
                        sf: 0x5a,
                        of: 0x5b,
                        ..initial.flags.bytes
                    };
                    for predicate in [0, 1] {
                        let mut expected = initial;
                        if predicate == 1 {
                            [
                                expected.flags.bytes.cf,
                                expected.flags.bytes.pf,
                                expected.flags.bytes.af,
                                expected.flags.bytes.zf,
                                expected.flags.bytes.sf,
                                expected.flags.bytes.of,
                            ] = if !partial {
                                [carry, 1, 1, 1, 0, 0]
                            } else if pending_base {
                                [carry, carry, 1 - carry, carry, 1 - carry, 0]
                            } else {
                                [carry, 1, 0, 1, 0, 0]
                            };
                        } else if pending_base {
                            expected.flags.status_source.kind = 2;
                            expected.flags.status_source.left = if carry == 0 { 127 } else { 128 };
                            expected.flags.status_source.right = if carry == 0 { 1 } else { 128 };
                        }
                        expected.eip = 0x1002;
                        expected.instruction_count = 0;
                        let result = i64::from(carry) * 2 + i64::from(predicate == 0);
                        assert_result(&module, &initial, &[predicate], &expected, result);
                    }
                }
            }
        }
    }
}
