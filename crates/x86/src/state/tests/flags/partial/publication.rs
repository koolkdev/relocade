use crate::alu::flags::{Condition, FlagChange, FlagSource, StatusFlag};
use crate::alu::ArithmeticOp;
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::{CompiledModule, StatusFlags};
use wasm86_compiler::{
    BuildError, FunctionBuilder, Program, Signature, Type, Val, I1, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, Validator};

use super::super::fixture::{assert_result, initial_cpu};

const SUB_FLAGS: StatusFlags = StatusFlags {
    cf: 1,
    pf: 1,
    af: 1,
    zf: 0,
    sf: 1,
    of: 0,
};
const ZERO_FLAGS: StatusFlags = StatusFlags {
    cf: 0,
    pf: 1,
    af: 0,
    zf: 1,
    sf: 0,
    of: 0,
};

fn query_all(
    body: &mut FunctionBuilder<'_>,
    state: &mut State<'_>,
) -> Result<Val<I64>, BuildError> {
    let mut packed: Val<I64> = 0_u64.into();
    for code in 0..16_u8 {
        let value = state.condition(body, Condition::from_code(code))?;
        packed = packed.or(value.unsigned().extend::<I64>().shl(u32::from(code)));
    }
    Ok(packed)
}

fn condition_bits(flags: StatusFlags) -> i64 {
    let cf = flags.cf != 0;
    let pf = flags.pf != 0;
    let zf = flags.zf != 0;
    let sf = flags.sf != 0;
    let of = flags.of != 0;
    let mut packed = 0_i64;
    for (index, condition) in [of, cf, zf, cf || zf, sf, pf, sf != of, zf || sf != of]
        .into_iter()
        .enumerate()
    {
        packed |= i64::from(condition) << (index * 2);
        packed |= i64::from(!condition) << (index * 2 + 1);
    }
    packed
}

fn consumer_ops(bytes: &[u8]) -> Vec<Operator<'_>> {
    Validator::new().validate_all(bytes).unwrap();
    Parser::new(0)
        .parse_all(bytes)
        .find_map(|payload| {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                Some(
                    body.get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .map(Result::unwrap)
                        .collect(),
                )
            } else {
                None
            }
        })
        .unwrap()
}

#[test]
fn mixed_histories_preserve_all_conditions_records_and_earlier_publications() {
    for local_base in [false, true] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I1; 5],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let mut state = State::new(&cpu);
                    if local_base {
                        state.set_flags(
                            &mut body,
                            ArithmeticOp::Subtract.apply::<I32>(4, 5).flags,
                        )?;
                    }
                    let first = body.parameter::<I1>(0)?;
                    state.set_flags_if(
                        &mut body,
                        first,
                        FlagChange::partial([
                            (StatusFlag::CF, false.into()),
                            (StatusFlag::OF, true.into()),
                        ]),
                    )?;
                    let stop = body.parameter::<I1>(4)?;
                    body.if_(stop, |mut arm| {
                        // Publication may resolve stored flags inside this child arm.
                        // Subsequent parent queries must not reuse descendant-only values.
                        state.publish(&mut arm, 0x1002, 1)?;
                        arm.return_(0_u64)
                    })?;
                    let second = body.parameter::<I1>(1)?;
                    state.set_flags_if(
                        &mut body,
                        second,
                        FlagSource::<I8>::Logic { result: 0.into() },
                    )?;
                    let third = body.parameter::<I1>(2)?;
                    state.set_flags_if(
                        &mut body,
                        third,
                        FlagChange::partial([
                            (StatusFlag::ZF, false.into()),
                            (StatusFlag::PF, false.into()),
                        ]),
                    )?;
                    let fourth = body.parameter::<I1>(3)?;
                    state.set_flags_if(
                        &mut body,
                        fourth,
                        FlagChange::partial([
                            (StatusFlag::AF, false.into()),
                            (StatusFlag::SF, true.into()),
                        ]),
                    )?;
                    let conditions = query_all(&mut body, &mut state)?;
                    state.publish(&mut body, 0x1008, 4)?;
                    body.return_(conditions)
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
            for mask in 0..16 {
                let arguments = std::array::from_fn::<_, 4, _>(|index| (mask >> index) & 1);
                let [first, second, third, fourth] = arguments;
                for stop in [0, 1] {
                    let mut expected = initial;
                    let mut status = if local_base || kind == 9 {
                        SUB_FLAGS
                    } else {
                        StatusFlags {
                            cf: 1,
                            pf: 1,
                            af: 1,
                            zf: 1,
                            sf: 1,
                            of: 1,
                        }
                    };
                    let mut concrete = first == 1;
                    if first == 1 {
                        status.cf = 0;
                        status.of = 1;
                    }
                    let active_second = second == 1 && stop == 0;
                    if active_second {
                        status = ZERO_FLAGS;
                        concrete = false;
                    }
                    if third == 1 && stop == 0 {
                        status.zf = 0;
                        status.pf = 0;
                        concrete = true;
                    }
                    if fourth == 1 && stop == 0 {
                        status.af = 0;
                        status.sf = 1;
                        concrete = true;
                    }
                    if concrete {
                        expected.flags.kind = 0;
                        expected.flags.status = status;
                    } else if active_second {
                        expected.flags.kind = 3;
                        expected.flags.left = 0;
                    } else if local_base {
                        expected.flags.kind = 9;
                        expected.flags.left = 4;
                        expected.flags.right = 5;
                    }
                    expected.eip = if stop == 1 { 0x1002 } else { 0x1008 };
                    expected.instruction_count = if stop == 1 { 0 } else { 3 };
                    assert_result(
                        &module,
                        &initial,
                        &[first, second, third, fourth, stop],
                        &expected,
                        if stop == 1 { 0 } else { condition_bits(status) },
                    );
                }
            }
        }
    }
}

#[test]
fn overwritten_partial_changes_do_not_generate_obsolete_stored_reader_calls() {
    for complete in [false, true] {
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
                    let pending = body.parameter::<I1>(0)?;
                    state.set_flags_if(
                        &mut body,
                        pending,
                        FlagChange::partial([(StatusFlag::CF, true.into())]),
                    )?;
                    if complete {
                        state.set_flags(&mut body, FlagSource::<I8>::Logic { result: 0.into() })?;
                    } else {
                        // Independent unconditional writes define every bit without
                        // requiring any part of the stored or conditional old source.
                        for (flag, value) in StatusFlag::ALL
                            .into_iter()
                            .zip([false, true, false, true, false, false])
                        {
                            state.set_flags(
                                &mut body,
                                FlagChange::partial([(flag, value.into())]),
                            )?;
                        }
                    }
                    let conditions = query_all(&mut body, &mut state)?;
                    state.publish(&mut body, 0x1002, 1)?;
                    body.return_(conditions)
                },
            )
            .unwrap();
        program.export("run", function).unwrap();
        let bytes = program.compile().unwrap();
        assert!(!consumer_ops(&bytes)
            .iter()
            .any(|operation| matches!(operation, Operator::Call { .. })));
        let module = TestModule::new(&CompiledModule {
            bytes,
            entry: "run".into(),
        });
        let initial = initial_cpu();
        let mut expected = initial;
        if complete {
            expected.flags.kind = 3;
            expected.flags.left = 0;
        } else {
            expected.flags.kind = 0;
            expected.flags.status = ZERO_FLAGS;
        }
        expected.eip = 0x1002;
        expected.instruction_count = 0;
        for pending in [0, 1] {
            assert_result(
                &module,
                &initial,
                &[pending],
                &expected,
                condition_bits(ZERO_FLAGS),
            );
        }
    }
}

#[test]
fn mixed_partial_and_complete_history_keeps_constant_publication_depth() {
    for length in [8_u32, 256] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let mut state = State::new(&cpu);
                    state.set_flags(&mut body, FlagSource::<I8>::Logic { result: 0.into() })?;
                    let selector = body.parameter::<I32>(0)?;
                    for index in 0..length {
                        let predicate = selector.eq(index);
                        if index % 2 == 0 {
                            state.set_flags_if(
                                &mut body,
                                predicate,
                                FlagSource::<I32>::Logic {
                                    result: index.into(),
                                },
                            )?;
                        } else {
                            state.set_flags_if(
                                &mut body,
                                predicate,
                                FlagChange::partial([
                                    (StatusFlag::CF, (index & 2 != 0).into()),
                                    (StatusFlag::OF, (index & 4 != 0).into()),
                                ]),
                            )?;
                        }
                    }
                    let conditions = query_all(&mut body, &mut state)?;
                    state.publish(&mut body, 0x1200, length)?;
                    body.return_(conditions)
                },
            )
            .unwrap();
        program.export("run", function).unwrap();
        let bytes = program.compile().unwrap();
        let mut depth = 0;
        let mut maximum = 0;
        for operation in consumer_ops(&bytes) {
            match operation {
                Operator::Block { .. } | Operator::If { .. } | Operator::Loop { .. } => depth += 1,
                Operator::End if depth > 0 => depth -= 1,
                _ => {}
            }
            maximum = maximum.max(depth);
        }
        assert_eq!(
            maximum, 2,
            "publication depth must not grow with {length} changes"
        );
        let module = TestModule::new(&CompiledModule {
            bytes,
            entry: "run".into(),
        });
        let initial = initial_cpu();
        for selector in [-1, 0, 1, length as i32 - 2, length as i32 - 1] {
            let mut expected = initial;
            let mut status = ZERO_FLAGS;
            if selector < 0 {
                expected.flags.kind = 3;
                expected.flags.left = 0;
            } else if selector % 2 == 0 {
                expected.flags.kind = 11;
                expected.flags.left = selector as u32;
                status.pf = u8::from((selector as u8).count_ones() % 2 == 0);
                status.zf = u8::from(selector == 0);
            } else {
                status.cf = u8::from(selector & 2 != 0);
                status.of = u8::from(selector & 4 != 0);
                expected.flags.kind = 0;
                expected.flags.status = status;
            }
            expected.eip = 0x1200;
            expected.instruction_count = length - 1;
            assert_result(
                &module,
                &initial,
                &[selector],
                &expected,
                condition_bits(status),
            );
        }
    }
}
