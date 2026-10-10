use super::*;
use crate::test_step::Engine;

fn probe_module(constant_bytes: Option<u32>, intent: Intent) -> TestModule {
    let mut program = Program::new();
    let memory = Memory::declare(
        &mut program,
        crate::ExecutionProfile::Protected(crate::SegmentProfile::Flat32),
        false,
    )
    .unwrap();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I32; 4],
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
                    access.scattered().unsigned().extend::<I32>(),
                    access.unavailable.unsigned().extend::<I32>(),
                    access.physical,
                ))
            },
        )
        .unwrap();
    program.export("probe", function).unwrap();
    TestModule::new(&crate::CompiledModule {
        execution_profile: None,
        bytes: program.compile().unwrap(),
        entry: "probe".into(),
    })
}

fn check_probes(engine: Engine) {
    for intent in [Intent::Read, Intent::Write, Intent::Fetch] {
        let mut modules = std::collections::BTreeMap::new();
        for (start, bytes, entries, read_denied, write_denied, scattered) in [
            (0x4fffu32, 1, [0x8003u32, 0, 0], false, false, false),
            (0x4ffe, 2, [0x8003, 0, 0], false, false, false),
            (0x4000, 4096, [0x8003, 0, 0], false, false, false),
            (0x4001, 4096, [0x8003, 0x9003, 0], false, false, false),
            (0x4ffc, 6, [0x8003, 0xa003, 0], false, false, true),
            (0x4fff, 2, [0x8ffd, 0x9fff, 0], false, true, false),
            (0x4fff, 2, [0x8fff, 0x9ffd, 0], false, true, false),
            (0x4fff, 2, [0x8000, 0x9003, 0], true, true, false),
            (0x4fff, 2, [0x8003, 0x9000, 0], true, true, false),
            (0x4fff, 2, [0x8000, 0xa000, 0], true, true, false),
            (0x4001, 1, [0x8000, 0x9003, 0], true, true, false),
            (0xffff_ffff, 2, [0x8003, 0x9003, 0], false, false, false),
            (0xffff_fffc, 6, [0x8003, 0xa003, 0], false, false, true),
            (0x4fff, 2, [0xffff_f003, 3, 0], false, false, true),
            (0x4000, 4097, [0x8003, 0, 0], true, true, false),
            (0x4ffe, 4098, [0x8003, 0x9003, 0], false, false, false),
            (0x4ffe, 4099, [0x8003, 0x9003, 0], true, true, false),
            (0x4ffe, 4100, [0x8003, 0x9003, 0xa003], false, false, false),
            (0x4ffe, 4100, [0x8003, 0xb003, 0xa003], false, false, true),
            (0x4ffe, 4100, [0x8003, 0, 0xa003], true, true, false),
            (0x4ffe, 4100, [0x8fff, 0x9ffd, 0xafff], false, true, false),
            (0x4ffe, 4100, [0x8003, 0xb003, 0], true, true, false),
            (0x4ffe, 0, [0x8003, 0x9003, 0xa003], true, true, false),
            (0x4001, u32::MAX, [0x8003, 0, 0], true, true, false),
            (0x4003, u32::MAX, [0x8003, 0, 0], true, true, false),
            (
                0xffff_fffe,
                4100,
                [0x8003, 0x9003, 0xa003],
                false,
                false,
                false,
            ),
        ] {
            let denied = match intent {
                Intent::Read | Intent::Fetch => read_denied,
                Intent::Write => write_denied,
            };
            let mut input = Input::new(&CpuState::filled(0xa5).to_bytes());
            input.arguments = vec![Argument::I32(start as i32), Argument::I32(bytes as i32)];
            input.observe_guest = true;
            for (offset, entry) in entries.into_iter().enumerate() {
                let page = (start >> 12).wrapping_add(offset as u32) & 0xfffff;
                input.machine.push((page * 4, entry.to_le_bytes().to_vec()));
            }
            let physical = (entries[0] & !0xfff) | (start & 0xfff);
            for constant_bytes in [Some(bytes), None] {
                let module = modules
                    .entry(constant_bytes)
                    .or_insert_with(|| probe_module(constant_bytes, intent));
                assert_eq!(
                    engine.observe(module, &input, 1),
                    Observation {
                        events: vec![Event::Return {
                            outcome: Outcome::Returned(vec![
                                Argument::I32(i32::from(denied)),
                                Argument::I32(i32::from(scattered)),
                                Argument::I32(i32::from(denied || scattered)),
                                Argument::I32(physical as i32),
                            ]),
                            snapshot: Snapshot {
                                cpu: input.cpu.clone(),
                                guest: Some(Vec::new()),
                            },
                        }],
                        guest_unchanged: true,
                        machine_unchanged: true,
                    },
                    "start {start:x}, bytes {bytes}, entries {entries:x?}, constant {constant_bytes:?}"
                );
            }
        }
    }
}

#[test]
fn probes_report_permissions_and_contiguity_without_transferring_guest_bytes() {
    check_probes(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_probes_report_permissions_and_contiguity_without_transferring_guest_bytes() {
    check_probes(Engine::V8);
}

#[test]
fn short_constant_probes_inline_checks_and_other_ranges_share_the_walker() {
    for (bytes, expected_calls) in [(Some(1), 0), (Some(6), 0), (None, 1), (Some(4100), 1)] {
        let module = probe_module(bytes, Intent::Read);
        let mut entry_index = None;
        let mut function_index = 0;
        let mut calls = None;
        for payload in Parser::new(0).parse_all(module.bytes()) {
            match payload.unwrap() {
                Payload::ExportSection(exports) => {
                    entry_index = Some(exports.into_iter().next().unwrap().unwrap().index);
                }
                Payload::CodeSectionEntry(body) => {
                    if Some(function_index) == entry_index {
                        calls = Some(
                            body.get_operators_reader()
                                .unwrap()
                                .into_iter()
                                .filter(|op| matches!(op, Ok(Operator::Call { .. })))
                                .count(),
                        );
                    }
                    function_index += 1;
                }
                _ => {}
            }
        }
        assert_eq!(calls, Some(expected_calls));
    }
}
