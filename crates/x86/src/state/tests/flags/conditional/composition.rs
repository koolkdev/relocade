use crate::alu::flags::{Condition, FlagChange, StatusFlag};
use crate::alu::ArithmeticOp;
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::{CompiledModule, StatusFlags};
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
                            state.set_flags(
                                &mut body,
                                ArithmeticOp::Subtract.apply::<I32>(4, 5).flags,
                            )?;
                        }
                        let first = body.parameter::<I1>(0)?;
                        let second = body.parameter::<I1>(1)?;
                        let change = if partial {
                            FlagChange::partial([
                                (StatusFlag::CF, false.into()),
                                (StatusFlag::OF, true.into()),
                            ])
                        } else {
                            ArithmeticOp::Add.apply::<I8>(127, 1).flags
                        };
                        state.set_flags(&mut body, change.when(first).when(second))?;
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
                initial.flags.kind = kind;
                for (first, second, active) in
                    [(0, 0, false), (0, 1, false), (1, 0, false), (1, 1, true)]
                {
                    let mut expected = initial;
                    if active && partial {
                        expected.flags.kind = 0;
                        expected.flags.status = StatusFlags {
                            cf: 0,
                            pf: 1,
                            af: 1,
                            zf: u8::from(kind == 0 && !pending_base),
                            sf: 1,
                            of: 1,
                        };
                    } else if active {
                        expected.flags.kind = 2;
                        expected.flags.left = 127;
                        expected.flags.right = 1;
                    } else if pending_base {
                        expected.flags.kind = 9;
                        expected.flags.left = 4;
                        expected.flags.right = 5;
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
                                    state.set_flags(
                                        &mut body,
                                        ArithmeticOp::Add.apply::<I8>(left, right).flags,
                                    )?;
                                }
                                let predicate = body.parameter::<I1>(0)?;
                                let change = if partial {
                                    FlagChange::partial([
                                        (StatusFlag::CF, false.into()),
                                        (StatusFlag::OF, false.into()),
                                    ])
                                } else {
                                    ArithmeticOp::Add.apply::<I8>(255, 1).flags
                                };
                                let change = if condition_first {
                                    change.when(predicate).preserving(StatusFlag::CF)
                                } else {
                                    change.preserving(StatusFlag::CF).when(predicate)
                                };
                                state.set_flags(&mut body, change)?;
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
                    initial.flags.kind = 0;
                    initial.flags.status = StatusFlags {
                        cf: 0xfe | if pending_base { 1 - carry } else { carry },
                        pf: 0x7f,
                        af: 0x80,
                        zf: 0xff,
                        sf: 0x5a,
                        of: 0x5b,
                    };
                    for predicate in [0, 1] {
                        let mut expected = initial;
                        if predicate == 1 {
                            expected.flags.status = if !partial {
                                StatusFlags {
                                    cf: carry,
                                    pf: 1,
                                    af: 1,
                                    zf: 1,
                                    sf: 0,
                                    of: 0,
                                }
                            } else if pending_base {
                                StatusFlags {
                                    cf: carry,
                                    pf: carry,
                                    af: 1 - carry,
                                    zf: carry,
                                    sf: 1 - carry,
                                    of: 0,
                                }
                            } else {
                                StatusFlags {
                                    cf: carry,
                                    pf: 1,
                                    af: 0,
                                    zf: 1,
                                    sf: 0,
                                    of: 0,
                                }
                            };
                        } else if pending_base {
                            expected.flags.kind = 2;
                            expected.flags.left = if carry == 0 { 127 } else { 128 };
                            expected.flags.right = if carry == 0 { 1 } else { 128 };
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
