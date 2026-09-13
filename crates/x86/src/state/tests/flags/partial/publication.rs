use crate::alu::ArithmeticOp;
use crate::alu::StatusSource;
use crate::flags::{Condition, FlagChange, StatusFlag};
use crate::state::{Cpu, State};
use crate::test_step::TestModule;
use crate::{CompiledModule, FlagBytes};
use wasm86_compiler::{
    BuildError, FunctionBuilder, Program, Signature, Type, Val, I1, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, Validator};

use super::super::fixture::{assert_result, initial_cpu};

const SUB_FLAGS: [u8; 6] = [1, 1, 1, 0, 1, 0];
const ZERO_FLAGS: [u8; 6] = [0, 1, 0, 1, 0, 0];

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

fn condition_bits(flags: [u8; 6]) -> i64 {
    let [cf, pf, _, zf, sf, of] = flags.map(|flag| flag != 0);
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
                        state.write_flags(
                            &mut body,
                            ArithmeticOp::Subtract.apply::<I32>(4, 5).flags,
                        )?;
                    }
                    let first = body.parameter::<I1>(0)?;
                    state.write_flags(
                        &mut body,
                        FlagChange::partial([
                            (StatusFlag::CF.into(), false.into()),
                            (StatusFlag::OF.into(), true.into()),
                        ])
                        .when(first),
                    )?;
                    let stop = body.parameter::<I1>(4)?;
                    body.if_(stop, |mut arm| {
                        // Publication may resolve stored flags inside this child arm.
                        // Subsequent parent queries must not reuse descendant-only values.
                        state.publish(&mut arm, 0x1002, 1)?;
                        arm.return_(0_u64)
                    })?;
                    let second = body.parameter::<I1>(1)?;
                    state.write_flags(
                        &mut body,
                        FlagChange::from(StatusSource::<I8>::Logic { result: 0.into() })
                            .when(second),
                    )?;
                    let third = body.parameter::<I1>(2)?;
                    state.write_flags(
                        &mut body,
                        FlagChange::partial([
                            (StatusFlag::ZF.into(), false.into()),
                            (StatusFlag::PF.into(), false.into()),
                        ])
                        .when(third),
                    )?;
                    let fourth = body.parameter::<I1>(3)?;
                    state.write_flags(
                        &mut body,
                        FlagChange::partial([
                            (StatusFlag::AF.into(), false.into()),
                            (StatusFlag::SF.into(), true.into()),
                        ])
                        .when(fourth),
                    )?;
                    let conditions = query_all(&mut body, &mut state)?;
                    state.publish(&mut body, 0x1008, 4)?;
                    body.return_(conditions)
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
            let mut initial = initial_cpu();
            initial.flags.status_source.kind = kind;
            for mask in 0..16 {
                let arguments = std::array::from_fn::<_, 4, _>(|index| (mask >> index) & 1);
                let [first, second, third, fourth] = arguments;
                for stop in [0, 1] {
                    let mut expected = initial;
                    let [mut cf, mut pf, mut af, mut zf, mut sf, mut of] =
                        if local_base || kind == 9 {
                            SUB_FLAGS
                        } else {
                            [1, 1, 1, 1, 1, 1]
                        };
                    let mut concrete = first == 1;
                    if first == 1 {
                        cf = 0;
                        of = 1;
                    }
                    let active_second = second == 1 && stop == 0;
                    if active_second {
                        [cf, pf, af, zf, sf, of] = ZERO_FLAGS;
                        concrete = false;
                    }
                    if third == 1 && stop == 0 {
                        zf = 0;
                        pf = 0;
                        concrete = true;
                    }
                    if fourth == 1 && stop == 0 {
                        af = 0;
                        sf = 1;
                        concrete = true;
                    }
                    if concrete {
                        expected.flags.status_source.kind = 0;
                        expected.flags.bytes = FlagBytes {
                            cf,
                            pf,
                            af,
                            zf,
                            sf,
                            of,
                            ..expected.flags.bytes
                        };
                    } else if active_second {
                        expected.flags.status_source.kind = 3;
                        expected.flags.status_source.left = 0;
                    } else if local_base {
                        expected.flags.status_source.kind = 9;
                        expected.flags.status_source.left = 4;
                        expected.flags.status_source.right = 5;
                    }
                    expected.eip = if stop == 1 { 0x1002 } else { 0x1008 };
                    expected.instruction_count = if stop == 1 { 0 } else { 3 };
                    assert_result(
                        &module,
                        &initial,
                        &[first, second, third, fourth, stop],
                        &expected,
                        if stop == 1 {
                            0
                        } else {
                            condition_bits([cf, pf, af, zf, sf, of])
                        },
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
                    state.write_flags(
                        &mut body,
                        FlagChange::partial([(StatusFlag::CF.into(), true.into())]).when(pending),
                    )?;
                    if complete {
                        state.write_flags(
                            &mut body,
                            StatusSource::<I8>::Logic { result: 0.into() },
                        )?;
                    } else {
                        // Independent unconditional writes define every bit without
                        // requiring any part of the stored or conditional old source.
                        for (flag, value) in StatusFlag::ALL
                            .into_iter()
                            .zip([false, true, false, true, false, false])
                        {
                            state.write_flags(
                                &mut body,
                                FlagChange::partial([(flag.into(), value.into())]),
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
            segment_profile: None,
            bytes,
            entry: "run".into(),
        });
        let initial = initial_cpu();
        let mut expected = initial;
        if complete {
            expected.flags.status_source.kind = 3;
            expected.flags.status_source.left = 0;
        } else {
            expected.flags.status_source.kind = 0;
            [
                expected.flags.bytes.cf,
                expected.flags.bytes.pf,
                expected.flags.bytes.af,
                expected.flags.bytes.zf,
                expected.flags.bytes.sf,
                expected.flags.bytes.of,
            ] = ZERO_FLAGS;
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
                    state.write_flags(&mut body, StatusSource::<I8>::Logic { result: 0.into() })?;
                    let selector = body.parameter::<I32>(0)?;
                    for index in 0..length {
                        let predicate = selector.eq(index);
                        if index % 2 == 0 {
                            state.write_flags(
                                &mut body,
                                FlagChange::from(StatusSource::<I32>::Logic {
                                    result: index.into(),
                                })
                                .when(predicate),
                            )?;
                        } else {
                            state.write_flags(
                                &mut body,
                                FlagChange::partial([
                                    (StatusFlag::CF.into(), (index & 2 != 0).into()),
                                    (StatusFlag::OF.into(), (index & 4 != 0).into()),
                                ])
                                .when(predicate),
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
            segment_profile: None,
            bytes,
            entry: "run".into(),
        });
        let initial = initial_cpu();
        for selector in [-1, 0, 1, length as i32 - 2, length as i32 - 1] {
            let mut expected = initial;
            let [mut cf, mut pf, af, mut zf, sf, mut of] = ZERO_FLAGS;
            if selector < 0 {
                expected.flags.status_source.kind = 3;
                expected.flags.status_source.left = 0;
            } else if selector % 2 == 0 {
                expected.flags.status_source.kind = 11;
                expected.flags.status_source.left = selector as u32;
                pf = u8::from((selector as u8).count_ones() % 2 == 0);
                zf = u8::from(selector == 0);
            } else {
                cf = u8::from(selector & 2 != 0);
                of = u8::from(selector & 4 != 0);
                expected.flags.status_source.kind = 0;
                expected.flags.bytes = FlagBytes {
                    cf,
                    pf,
                    af,
                    zf,
                    sf,
                    of,
                    ..expected.flags.bytes
                };
            }
            expected.eip = 0x1200;
            expected.instruction_count = length - 1;
            assert_result(
                &module,
                &initial,
                &[selector],
                &expected,
                condition_bits([cf, pf, af, zf, sf, of]),
            );
        }
    }
}
