use super::*;

fn checked_range(intent: Intent, constant_bytes: Option<u32>) -> TestModule {
    let mut program = Program::new();
    let memory = Memory::declare(
        &mut program,
        crate::ExecutionProfile::Protected(crate::SegmentProfile::Flat32),
    )
    .unwrap();
    let function = program
        .raw_function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: relocade_compiler::abi::Function::<fn() -> AccessResult>::new(
                    "test", "access",
                )
                .signature()
                .results,
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
                    Some(&mut AccessResult::fault),
                )?;
                AccessResult::value(body, 7)
            },
        )
        .unwrap();
    program.export("checked", function).unwrap();
    TestModule::new(&crate::CompiledModule {
        execution_target: None,
        bytes: program.compile().unwrap(),
        entry: "checked".into(),
    })
}

fn expect_resolution(module: &TestModule, input: &Input, expected: AccessResult) {
    assert_eq!(
        module.observe(input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: returned(expected),
                snapshot: Snapshot {
                    cpu: input.cpu.clone(),
                    guest: Some(Vec::new()),
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "arguments {:?}, expected {expected:?}",
        input.arguments,
    );
}

#[test]
fn faulting_ranges_report_the_first_denied_page_even_after_linear_wrap() {
    for intent in [
        Intent::Read,
        Intent::Write,
        Intent::Fetch,
        Intent::ReadOrExecute,
    ] {
        let missing = match intent {
            Intent::Read | Intent::ReadOrExecute => 0,
            Intent::Write => 2,
            Intent::Fetch => 0x10,
        };
        for constant_bytes in [None, Some(4100)] {
            let module = checked_range(intent, constant_bytes);
            for start in [0x4ffeu32, 0xffff_fffe] {
                let second_page = start.wrapping_add(2);
                let third_page = second_page.wrapping_add(4096);
                for (entries, expected) in [
                    ([0u32, 0, 0xa003], page_fault(start, missing)),
                    ([0x8003, 0, 0], page_fault(second_page, missing)),
                    // Discontinuity does not stop validation of later pages.
                    ([0x8003, 0xb003, 0], page_fault(third_page, missing)),
                    ([0x8003, 0xb003, 0xa003], AccessResult::Value(7)),
                    (
                        [0x8fff, 0x9ffd, 0xafff],
                        if matches!(intent, Intent::Write) {
                            page_fault(second_page, 3)
                        } else {
                            AccessResult::Value(7)
                        },
                    ),
                ] {
                    let mut input = Input {
                        arguments: vec![Argument::I32(start as i32), Argument::I32(4100)],
                        observe_guest: true,
                        ..Input::new(&CpuState::filled(0xa5).to_bytes())
                    };
                    for (offset, entry) in entries.into_iter().enumerate() {
                        let page = ((start >> 12) + offset as u32) & 0xfffff;
                        input.machine.push((page * 4, entry.to_le_bytes().to_vec()));
                    }
                    expect_resolution(&module, &input, expected);
                }
            }
        }
    }
}

#[test]
fn ranges_near_four_gib_count_every_page_without_overflow() {
    let module = checked_range(Intent::Read, None);
    // All linear pages alias valid backing. Leave page 3 absent so a span can
    // stop immediately before it, or report it after traversing the address wrap.
    let mut table = 0x8003u32.to_le_bytes().repeat(1 << 20);
    table[12..16].fill(0);
    let mut input = Input {
        machine: vec![(0, table)],
        observe_guest: true,
        ..Input::new(&CpuState::filled(0xa5).to_bytes())
    };
    for (start, bytes, expected) in [
        (0x4000u32, 0xffff_f000u32, AccessResult::Value(7)),
        (0x4000, 0xffff_f001, page_fault(0x3000, 0)),
        (0x4001, 0xffff_efff, AccessResult::Value(7)),
        (0x4001, 0xffff_f000, page_fault(0x3000, 0)),
    ] {
        input.arguments = vec![Argument::I32(start as i32), Argument::I32(bytes as i32)];
        expect_resolution(&module, &input, expected);
    }
    // The largest representable span can revisit its starting page after wrap.
    input.machine[0].1[12..16].copy_from_slice(&0x8003u32.to_le_bytes());
    for start in [0x4000, 0x4001, 0x4fff] {
        input.arguments = vec![Argument::I32(start), Argument::I32(-1)];
        expect_resolution(&module, &input, AccessResult::Value(7));
    }
}
