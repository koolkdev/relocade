use super::*;
use crate::test_step::Engine;

fn probe_module(intent: Intent, constant_bytes: Option<u32>) -> TestModule {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I32, Type::I32, Type::I32, Type::I32],
            },
            |mut body| {
                let start = body.parameter::<I32>(0)?;
                let bytes = match constant_bytes {
                    Some(bytes) => body.value::<I32>(bytes)?,
                    None => body.parameter::<I32>(1)?,
                };
                let access = memory.resolve_access(&mut body, &start, bytes, intent, None, None)?;
                body.return_((
                    access.denied.unsigned().extend::<I32>(),
                    access.scattered.unsigned().extend::<I32>(),
                    access.physical,
                    access.unavailable.unsigned().extend::<I32>(),
                ))
            },
        )
        .unwrap();
    program.export("probe", function).unwrap();
    TestModule::new(&crate::CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "probe".into(),
    })
}

fn check_ranges(engine: Engine) {
    for (intent, permissions) in [(Intent::Read, 1u32), (Intent::Write, 3)] {
        let mut modules = std::collections::BTreeMap::new();
        for (start, bytes, entries, denied, scattered) in [
            (0x4ffeu32, 1u32, [0x8000 | permissions, 0, 0], false, false),
            (0x4000, 4096, [0x8000 | permissions, 0, 0], false, false),
            (0x4000, 4097, [0x8000 | permissions, 0, 0], true, false),
            (
                0x4fff,
                4096,
                [0x8000 | permissions, 0x9000 | permissions, 0],
                false,
                false,
            ),
            (0x4ffe, 2, [0x8000 | permissions, 0, 0], false, false),
            (0x4ffe, 3, [0x8000 | permissions, 0, 0], true, false),
            (
                0x4ffe,
                3,
                [0x8000 | permissions, 0x9000 | permissions, 0],
                false,
                false,
            ),
            (
                0x4ffe,
                4100,
                [
                    0x8000 | permissions,
                    0x9000 | permissions,
                    0xa000 | permissions,
                ],
                false,
                false,
            ),
            (
                0x4ffe,
                4100,
                [0x8000 | permissions, 0, 0xa000 | permissions],
                true,
                false,
            ),
            (
                0x4ffe,
                4100,
                [
                    0x8000 | permissions,
                    0xb000 | permissions,
                    0xa000 | permissions,
                ],
                false,
                true,
            ),
            (
                0x4ffe,
                4100,
                [
                    0x8000 | permissions,
                    0x9000 | (permissions & !2),
                    0xa000 | permissions,
                ],
                permissions == 3,
                false,
            ),
            (
                0x4fff,
                2,
                [0xffff_f000 | permissions, permissions, 0],
                false,
                true,
            ),
            (0x4ffe, 0, [0x8000 | permissions, 0, 0], true, false),
            (0x4001, u32::MAX, [0x8000 | permissions, 0, 0], true, false),
            (
                0xffff_fffe,
                4,
                [0x8000 | permissions, 0x9000 | permissions, 0],
                false,
                false,
            ),
        ] {
            for constant_bytes in [None, Some(bytes)] {
                let module = modules
                    .entry(constant_bytes)
                    .or_insert_with(|| probe_module(intent, constant_bytes));
                let mut input = Input::new(&CpuState::filled(0xa5).to_bytes());
                input.arguments = vec![Argument::I32(start as i32), Argument::I32(bytes as i32)];
                for (i, entry) in entries.into_iter().enumerate() {
                    let page = ((start >> 12) + i as u32) & 0xfffff;
                    input.machine.push((page * 4, entry.to_le_bytes().to_vec()));
                }
                let physical = (entries[0] & !0xfff) | (start & 0xfff);
                assert_eq!(
                    engine.observe(module, &input, 1),
                    Observation {
                        events: vec![Event::Return {
                            outcome: Outcome::Returned(vec![
                                Argument::I32(i32::from(denied)),
                                Argument::I32(i32::from(scattered)),
                                Argument::I32(physical as i32),
                                Argument::I32(i32::from(denied || scattered)),
                            ]),
                            snapshot: Snapshot {
                                cpu: input.cpu.clone(),
                                guest: None
                            },
                        }],
                        guest_unchanged: true,
                        machine_unchanged: true,
                    },
                    "{start:x} + {bytes}, permissions {permissions}, constant {constant_bytes:?}"
                );
            }
        }
    }
}

#[test]
fn non_faulting_ranges_distinguish_denial_from_scattered_backing() {
    check_ranges(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_non_faulting_ranges_distinguish_denial_from_scattered_backing() {
    check_ranges(Engine::V8);
}

fn check_faulting_ranges(engine: Engine) {
    for (intent, permissions) in [(Intent::Read, 1u32), (Intent::Write, 3)] {
        for constant_bytes in [None, Some(4100)] {
            let mut program = Program::new();
            let memory = Memory::declare(&mut program).unwrap();
            let function = program
                .function(
                    Signature {
                        parameters: vec![Type::I32, Type::I32],
                        results: vec![Type::I64],
                    },
                    |mut body| {
                        let start = body.parameter::<I32>(0)?;
                        let bytes = match constant_bytes {
                            Some(bytes) => body.value::<I32>(bytes)?,
                            None => body.parameter::<I32>(1)?,
                        };
                        memory.resolve_access(
                            &mut body,
                            &start,
                            bytes,
                            intent,
                            None,
                            Some(&mut exit::exception),
                        )?;
                        body.return_(7)
                    },
                )
                .unwrap();
            program.export("checked", function).unwrap();
            let module = TestModule::new(&crate::CompiledModule {
                segment_profile: None,
                bytes: program.compile().unwrap(),
                entry: "checked".into(),
            });
            let missing_page: u64 = if permissions == 3 {
                0x0004_0002_0000_0000
            } else {
                0x0004_0000_0000_0000
            };
            for (entries, expected) in [
                (
                    [0, 0x9000 | permissions, 0xa000 | permissions],
                    missing_page | 0x4ffe,
                ),
                (
                    [0x8000 | permissions, 0, 0xa000 | permissions],
                    missing_page | 0x5000,
                ),
                // A scattered middle frame is allowed; a later denial still wins.
                (
                    [0x8000 | permissions, 0xb000 | permissions, 0],
                    missing_page | 0x6000,
                ),
                (
                    [
                        0x8000 | permissions,
                        0xb000 | permissions,
                        0xa000 | permissions,
                    ],
                    7,
                ),
                (
                    [0x8000 | permissions, 0x9001, 0xa000 | permissions],
                    if permissions == 3 {
                        0x0004_0003_0000_5000
                    } else {
                        7
                    },
                ),
            ] {
                let mut input = Input::new(&CpuState::filled(0xa5).to_bytes());
                input.arguments = vec![Argument::I32(0x4ffe), Argument::I32(4100)];
                for (page, entry) in entries.into_iter().enumerate() {
                    input
                        .machine
                        .push(((4 + page as u32) * 4, entry.to_le_bytes().to_vec()));
                }
                assert_eq!(
                    engine.observe(&module, &input, 1),
                    Observation {
                        events: vec![Event::Return {
                            outcome: Outcome::Returned(vec![Argument::I64(expected as i64)]),
                            snapshot: Snapshot {
                                cpu: input.cpu.clone(),
                                guest: None
                            },
                        }],
                        guest_unchanged: true,
                        machine_unchanged: true,
                    }
                );
            }
        }
    }
}

#[test]
fn large_faulting_ranges_report_the_first_denied_page() {
    check_faulting_ranges(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_large_faulting_ranges_report_the_first_denied_page() {
    check_faulting_ranges(Engine::V8);
}

#[test]
fn constant_short_ranges_keep_inline_checks_and_dynamic_ranges_share_the_handler() {
    for (constant_bytes, expected_calls) in [(Some(1), 0), (Some(3), 0), (None, 1), (Some(4100), 1)]
    {
        let module = probe_module(Intent::Read, constant_bytes);
        let mut entry_index = None;
        let mut function_index = 0;
        let mut calls = 0;
        for payload in Parser::new(0).parse_all(module.bytes()) {
            match payload.unwrap() {
                Payload::ExportSection(exports) => {
                    entry_index = Some(exports.into_iter().next().unwrap().unwrap().index);
                }
                Payload::CodeSectionEntry(body) => {
                    if Some(function_index) == entry_index {
                        calls = body
                            .get_operators_reader()
                            .unwrap()
                            .into_iter()
                            .filter(|op| matches!(op, Ok(Operator::Call { .. })))
                            .count();
                    }
                    function_index += 1;
                }
                _ => {}
            }
        }
        assert_eq!(calls, expected_calls);
    }
}
