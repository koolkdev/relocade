//! Generated queries read only requested flags and share missing status subsets.

use super::cached_reads_and_writes;
use crate::alu::ArithmeticOp;
use crate::flags::{Flag, FlagChange};
use crate::state::{Cpu, State};
use crate::CompiledModule;
use wasm86_compiler::{Program, Signature, Type, I1, I8};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, ValType, Validator};

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
    for (module, expected_loads) in [
        (
            stored_reader([Flag::SF, Flag::CF, Flag::AF, Flag::CF, Flag::ZF]),
            vec![],
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
            vec![19],
        ),
        (
            stored_reader([
                Flag::ID,
                Flag::SF,
                Flag::TF,
                Flag::CF,
                Flag::AC,
                Flag::AF,
                Flag::NT,
                Flag::DF,
                Flag::CF,
                Flag::ID,
                Flag::ZF,
            ]),
            vec![18, 19, 20, 21, 22],
        ),
    ] {
        let (calls, mut loads) = entry_shape(&module);
        assert_eq!(
            calls,
            [vec![ValType::I32; 4]],
            "duplicates and direct flags do not enlarge the four-status subset"
        );
        loads.sort_unstable();
        loads.dedup();
        assert_eq!(
            loads, expected_loads,
            "only requested direct bytes are read"
        );
    }
    for (flags, expected_loads) in [
        ([Flag::DF, Flag::DF], vec![19]),
        ([Flag::ID, Flag::ID], vec![22]),
        ([Flag::AC, Flag::TF], vec![18, 21]),
    ] {
        let (calls, mut loads) = entry_shape(&stored_reader(flags));
        assert!(
            calls.is_empty(),
            "direct-only requests need no status helper"
        );
        loads.sort_unstable();
        loads.dedup();
        assert_eq!(loads, expected_loads);
    }
    let (calls, _) = entry_shape(&cached_reads_and_writes());
    assert_eq!(calls, [vec![ValType::I32; 1], vec![ValType::I32; 3]], "single CF, then only the missing PF/AF/ZF subset; overlapping reads and local writes need no other helper");
}

#[test]
fn empty_and_fully_local_requests_need_no_helper_or_cpu_memory() {
    #[derive(Clone, Copy, Debug)]
    enum Request {
        Empty,
        LocalStatus,
        LocalStatusAndDirect,
        ExplicitSubset,
        ExplicitMixedSubset,
    }

    for request in [
        Request::Empty,
        Request::LocalStatus,
        Request::LocalStatusAndDirect,
        Request::ExplicitSubset,
        Request::ExplicitMixedSubset,
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
                        } else if matches!(request, Request::LocalStatusAndDirect) {
                            8
                        } else {
                            4
                        }
                    ],
                },
                |mut body| {
                    let mut state = State::new(&cpu);
                    let results = if matches!(request, Request::Empty) {
                        state.read_flags(&mut body, [])?.to_vec()
                    } else if matches!(request, Request::ExplicitMixedSubset) {
                        let trap_flag = body.parameter::<I1>(2)?;
                        state.write_flags(
                            &mut body,
                            FlagChange::partial([
                                (Flag::CF, true.into()),
                                (Flag::TF, trap_flag),
                                (Flag::ID, false.into()),
                            ]),
                        )?;
                        state
                            .read_flags(&mut body, [Flag::ID, Flag::CF, Flag::TF, Flag::ID])?
                            .to_vec()
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
                            // All direct flags remain stored but are not requested.
                            state
                                .read_flags(&mut body, [Flag::OF, Flag::AF, Flag::CF, Flag::AF])?
                                .to_vec()
                        } else {
                            let direct = body.parameter::<I1>(2)?;
                            state.write_flags(
                                &mut body,
                                FlagChange::partial(
                                    [Flag::TF, Flag::DF, Flag::NT, Flag::AC, Flag::ID]
                                        .map(|flag| (flag, direct.clone())),
                                ),
                            )?;
                            state
                                .read_flags(
                                    &mut body,
                                    [
                                        Flag::ID,
                                        Flag::TF,
                                        Flag::DF,
                                        Flag::NT,
                                        Flag::AC,
                                        Flag::OF,
                                        Flag::CF,
                                        Flag::ID,
                                    ],
                                )?
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
