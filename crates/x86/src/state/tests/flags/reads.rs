//! Ordered architectural flag queries share demand for stored status bits.

use super::fixture::initial_cpu;
use crate::alu::{ArithmeticOp, StatusSource};
use crate::flags::{Flag, FlagChange};
use crate::state::{Cpu, State};
use crate::test_step::{
    Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule,
};
use crate::{CompiledModule, CpuState};
use wasm86_compiler::{Program, Signature, Type, I1, I8};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, ValType, Validator};

fn assert_reads(
    engine: Engine,
    module: &TestModule,
    initial: &CpuState,
    arguments: &[i32],
    expected: &[i32],
) {
    let input = Input {
        arguments: arguments.iter().copied().map(Argument::I32).collect(),
        ..Input::new(&initial.to_bytes())
    };
    assert_eq!(
        engine.observe(module, &input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(expected.iter().copied().map(Argument::I32).collect()),
                snapshot: Snapshot {
                    cpu: initial.to_bytes().to_vec(),
                    guest: None,
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "arguments {arguments:?}"
    );
}

fn mixed_history(local_base: bool) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I1; 2],
                results: vec![Type::I1; 9],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                if local_base {
                    state.write_flags(&mut body, ArithmeticOp::Add.apply::<I8>(127, 1).flags)?;
                }
                state.write_flag(&mut body, Flag::ZF, true)?;
                let partial = body.parameter::<I1>(0)?;
                state.write_flags(
                    &mut body,
                    FlagChange::partial([(Flag::CF, false.into()), (Flag::DF, true.into())])
                        .when(partial),
                )?;
                let replace = body.parameter::<I1>(1)?;
                state.write_flags(
                    &mut body,
                    FlagChange::from(StatusSource::<I8>::Logic { result: 0.into() })
                        .preserving(Flag::AF)
                        .when(replace),
                )?;
                let flags = state.read_flags(
                    &mut body,
                    [
                        Flag::DF,
                        Flag::OF,
                        Flag::AF,
                        Flag::CF,
                        Flag::ZF,
                        Flag::DF,
                        Flag::PF,
                        Flag::SF,
                        Flag::CF,
                    ],
                )?;
                body.return_(flags.to_vec())
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_mixed_history(engine: Engine) {
    let mut initial = initial_cpu();
    initial.flags.bytes.df = 0xfe;
    // Stored SUB 7-8 has CF/PF/AF/SF set; local ADD 127+1 has AF/SF/OF set.
    for (local, rows) in [
        (
            false,
            [
                ([0, 0], [0, 0, 1, 1, 1, 0, 1, 1, 1]),
                ([1, 0], [1, 0, 1, 0, 1, 1, 1, 1, 0]),
                ([0, 1], [0, 0, 1, 0, 1, 0, 1, 0, 0]),
                ([1, 1], [1, 0, 1, 0, 1, 1, 1, 0, 0]),
            ],
        ),
        (
            true,
            [
                ([0, 0], [0, 1, 1, 0, 1, 0, 0, 1, 0]),
                ([1, 0], [1, 1, 1, 0, 1, 1, 0, 1, 0]),
                ([0, 1], [0, 0, 1, 0, 1, 0, 1, 0, 0]),
                ([1, 1], [1, 0, 1, 0, 1, 1, 1, 0, 0]),
            ],
        ),
    ] {
        let module = TestModule::new(&mixed_history(local));
        for (arguments, expected) in rows {
            assert_reads(engine, &module, &initial, &arguments, &expected);
        }
    }
}

#[test]
fn ordered_reads_compose_stored_local_partial_and_conditional_flags_without_publication() {
    check_mixed_history(Engine::Wasmtime);
}

fn cached_reads_and_writes() -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I1; 23],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let mut results = vec![state.read_flag(&mut body, Flag::CF)?];
                results.extend(state.read_flags(
                    &mut body,
                    [Flag::ZF, Flag::CF, Flag::DF, Flag::PF, Flag::CF, Flag::AF],
                )?);
                results.push(state.read_flag(&mut body, Flag::AF)?);
                results
                    .extend(state.read_flags(&mut body, [Flag::PF, Flag::AF, Flag::CF, Flag::ZF])?);
                state.write_flags(
                    &mut body,
                    FlagChange::partial([
                        (Flag::CF, false.into()),
                        (Flag::ZF, true.into()),
                        (Flag::DF, false.into()),
                    ]),
                )?;
                results.extend(state.read_flags(
                    &mut body,
                    [Flag::CF, Flag::ZF, Flag::AF, Flag::DF, Flag::CF],
                )?);
                state.write_flags(&mut body, ArithmeticOp::Add.apply::<I8>(127, 1).flags)?;
                results.extend(state.read_flags(
                    &mut body,
                    [Flag::OF, Flag::CF, Flag::ZF, Flag::PF, Flag::SF, Flag::DF],
                )?);
                body.return_(results)
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

fn check_cached_reads(engine: Engine) {
    let module = TestModule::new(&cached_reads_and_writes());
    let mut initial = initial_cpu();
    initial.flags.bytes.df = 0x81;
    assert_reads(
        engine,
        &module,
        &initial,
        &[],
        &[
            1, // Earlier single CF read.
            0, 1, 1, 1, 1, 1, // ZF, CF, DF, PF, CF, AF from the stored source.
            1, // Single AF read reuses the grouped result.
            1, 1, 1, 0, // Entirely cached PF, AF, CF, ZF.
            0, 1, 1, 0, 0, // Partial CF/ZF/DF writes retain AF.
            1, 0, 0, 0, 1, 0, // New arithmetic source retains the local DF write.
        ],
    );
}

#[test]
fn grouped_and_single_reads_share_cache_but_retain_values_across_later_writes() {
    check_cached_reads(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn grouped_flag_queries_in_v8() {
    check_mixed_history(Engine::V8);
    check_cached_reads(Engine::V8);
}

fn stored_reader<const N: usize>(flags: [Flag; N]) -> CompiledModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I1; N],
            },
            |mut body| {
                let mut state = State::new(&cpu);
                let values = state.read_flags(&mut body, flags)?;
                body.return_(values.to_vec())
            },
        )
        .unwrap();
    program.export("run", function).unwrap();
    CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "run".into(),
    }
}

// Inspect only helper boundaries and direct byte loads, not instruction ordering.
fn entry_shape(module: &CompiledModule) -> (Vec<Vec<ValType>>, Vec<u64>) {
    Validator::new().validate_all(&module.bytes).unwrap();
    let mut types = Vec::new();
    let mut functions = Vec::new();
    let mut entry = None;
    let mut calls = Vec::new();
    let mut loads = Vec::new();
    let mut body_index = 0;
    for payload in Parser::new(0).parse_all(&module.bytes) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                types.extend(section.into_iter_err_on_gc_types().map(Result::unwrap))
            }
            Payload::ImportSection(section) => {
                for import in section {
                    assert!(
                        !matches!(import.unwrap().ty, TypeRef::Func(_)),
                        "state readers do not import functions"
                    );
                }
            }
            Payload::FunctionSection(section) => {
                functions.extend(section.into_iter().map(Result::unwrap))
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.kind == ExternalKind::Func && export.name == module.entry {
                        entry = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                if Some(body_index) == entry {
                    for operator in body.get_operators_reader().unwrap() {
                        match operator.unwrap() {
                            Operator::Call { function_index } => calls.push(
                                types[functions[function_index as usize] as usize]
                                    .results()
                                    .to_vec(),
                            ),
                            Operator::I32Load8U { memarg } => loads.push(memarg.offset),
                            _ => {}
                        }
                    }
                }
                body_index += 1;
            }
            _ => {}
        }
    }
    assert!(entry.is_some());
    (calls, loads)
}

#[test]
fn uncached_status_flags_are_grouped_once_and_cached_overlap_requests_only_missing_flags() {
    for (module, direction) in [
        (
            stored_reader([Flag::SF, Flag::CF, Flag::AF, Flag::CF, Flag::ZF]),
            false,
        ),
        (
            stored_reader([
                Flag::DF,
                Flag::SF,
                Flag::CF,
                Flag::AF,
                Flag::DF,
                Flag::CF,
                Flag::ZF,
            ]),
            true,
        ),
    ] {
        let (calls, loads) = entry_shape(&module);
        assert_eq!(
            calls,
            [vec![ValType::I32; 4]],
            "duplicates and DF do not enlarge the four-status subset"
        );
        assert_eq!(
            loads.is_empty(),
            !direction,
            "DF is read directly only when requested"
        );
        assert!(
            loads.iter().all(|offset| *offset == 19),
            "the consumer directly loads only DF"
        );
    }
    let (calls, loads) = entry_shape(&stored_reader([Flag::DF, Flag::DF]));
    assert!(
        calls.is_empty(),
        "a DF-only request needs no stored status helper"
    );
    assert!(!loads.is_empty() && loads.iter().all(|offset| *offset == 19));
    let (calls, _) = entry_shape(&cached_reads_and_writes());
    assert_eq!(calls, [vec![ValType::I32; 1], vec![ValType::I32; 3]], "single CF, then only the missing PF/AF/ZF subset; overlapping reads and local writes need no other helper");
}

#[test]
fn empty_and_fully_local_requests_need_no_helper_or_cpu_memory() {
    #[derive(Clone, Copy, Debug)]
    enum Request {
        Empty,
        LocalStatus,
        LocalStatusAndDirection,
        ExplicitSubset,
    }

    for request in [
        Request::Empty,
        Request::LocalStatus,
        Request::LocalStatusAndDirection,
        Request::ExplicitSubset,
    ] {
        let mut program = Program::new();
        let cpu = Cpu::declare(&mut program);
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I8, Type::I8, Type::I1],
                    results: vec![
                        Type::I1;
                        if matches!(request, Request::Empty) {
                            0
                        } else {
                            4
                        }
                    ],
                },
                |mut body| {
                    let mut state = State::new(&cpu);
                    let results = if matches!(request, Request::Empty) {
                        state.read_flags(&mut body, [])?.to_vec()
                    } else if matches!(request, Request::ExplicitSubset) {
                        // The stored base is still needed for unrequested PF/ZF/SF.
                        let carry = body.parameter::<I1>(2)?;
                        state.write_flags(
                            &mut body,
                            FlagChange::partial([
                                (Flag::CF, carry),
                                (Flag::AF, false.into()),
                                (Flag::OF, true.into()),
                            ]),
                        )?;
                        state
                            .read_flags(&mut body, [Flag::OF, Flag::AF, Flag::CF, Flag::AF])?
                            .to_vec()
                    } else {
                        let left = body.parameter::<I8>(0)?;
                        let right = body.parameter::<I8>(1)?;
                        state.write_flags(&mut body, ArithmeticOp::Add.apply(left, right).flags)?;
                        if matches!(request, Request::LocalStatus) {
                            // DF remains stored but is not part of this request.
                            state
                                .read_flags(&mut body, [Flag::OF, Flag::AF, Flag::CF, Flag::AF])?
                                .to_vec()
                        } else {
                            let direction = body.parameter::<I1>(2)?;
                            state.write_flag(&mut body, Flag::DF, direction)?;
                            state
                                .read_flags(&mut body, [Flag::DF, Flag::OF, Flag::CF, Flag::DF])?
                                .to_vec()
                        }
                    };
                    body.return_(results)
                },
            )
            .unwrap();
        program.export("run", function).unwrap();
        let bytes = program.compile().unwrap();
        Validator::new().validate_all(&bytes).unwrap();
        let mut bodies = 0;
        for payload in Parser::new(0).parse_all(&bytes) {
            match payload.unwrap() {
                Payload::ImportSection(section) => assert_eq!(
                    section.count(),
                    0,
                    "{request:?} must not retain the CPU memory import"
                ),
                Payload::MemorySection(section) => assert_eq!(section.count(), 0),
                Payload::CodeSectionEntry(body) => {
                    bodies += 1;
                    for operator in body.get_operators_reader().unwrap() {
                        assert!(
                            !matches!(
                                operator.unwrap(),
                                Operator::Call { .. } | Operator::CallIndirect { .. }
                            ),
                            "{request:?} needs no helper"
                        );
                    }
                }
                _ => {}
            }
        }
        assert_eq!(bodies, 1, "{request:?} retains only its consumer");
    }
}
