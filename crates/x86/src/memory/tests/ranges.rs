use super::*;
use crate::test_step::Engine;

fn checked_range(intent: Intent, constant_bytes: Option<u32>) -> TestModule {
    let mut program = Program::new();
    let memory = Memory::declare(
        &mut program,
        crate::ExecutionProfile::Protected(crate::SegmentProfile::Flat32),
    )
    .unwrap();
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
    TestModule::new(&crate::CompiledModule {
        execution_profile: None,
        bytes: program.compile().unwrap(),
        entry: "checked".into(),
    })
}

fn expect_resolution(engine: Engine, module: &TestModule, input: &Input, expected: u64) {
    assert_eq!(
        engine.observe(module, input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(vec![Argument::I64(expected as i64)]),
                snapshot: Snapshot {
                    cpu: input.cpu.clone(),
                    guest: Some(Vec::new()),
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "arguments {:?}, expected {expected:x}",
        input.arguments,
    );
}

fn first_denial(engine: Engine) {
    for intent in [Intent::Read, Intent::Write, Intent::Fetch] {
        let missing = match intent {
            Intent::Read => 0x0004_0000_0000_0000,
            Intent::Write => 0x0004_0002_0000_0000,
            Intent::Fetch => 0x0004_0010_0000_0000,
        };
        for constant_bytes in [None, Some(4100)] {
            let module = checked_range(intent, constant_bytes);
            for start in [0x4ffeu32, 0xffff_fffe] {
                let second_page = start.wrapping_add(2);
                let third_page = second_page.wrapping_add(4096);
                for (entries, expected) in [
                    ([0u32, 0, 0xa003], missing | u64::from(start)),
                    ([0x8003, 0, 0], missing | u64::from(second_page)),
                    // Discontinuity does not stop validation of later pages.
                    ([0x8003, 0xb003, 0], missing | u64::from(third_page)),
                    ([0x8003, 0xb003, 0xa003], 7),
                    (
                        [0x8fff, 0x9ffd, 0xafff],
                        if matches!(intent, Intent::Write) {
                            0x0004_0003_0000_0000 | u64::from(second_page)
                        } else {
                            7
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
                    expect_resolution(engine, &module, &input, expected);
                }
            }
        }
    }
}

fn nearly_full_address_space(engine: Engine) {
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
        (0x4000u32, 0xffff_f000u32, 7),
        (0x4000, 0xffff_f001, 0x0004_0000_0000_3000),
        (0x4001, 0xffff_efff, 7),
        (0x4001, 0xffff_f000, 0x0004_0000_0000_3000),
    ] {
        input.arguments = vec![Argument::I32(start as i32), Argument::I32(bytes as i32)];
        expect_resolution(engine, &module, &input, expected);
    }
    // The largest representable span can revisit its starting page after wrap.
    input.machine[0].1[12..16].copy_from_slice(&0x8003u32.to_le_bytes());
    for start in [0x4000, 0x4001, 0x4fff] {
        input.arguments = vec![Argument::I32(start), Argument::I32(-1)];
        expect_resolution(engine, &module, &input, 7);
    }
}

#[test]
fn faulting_ranges_report_the_first_denied_page_even_after_linear_wrap() {
    first_denial(Engine::Wasmtime);
}

#[test]
fn ranges_near_four_gib_count_every_page_without_overflow() {
    nearly_full_address_space(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_faulting_ranges_report_the_first_denied_page_even_after_linear_wrap() {
    first_denial(Engine::V8);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_ranges_near_four_gib_count_every_page_without_overflow() {
    nearly_full_address_space(Engine::V8);
}
