//! Repeated accesses retain permissions and translations, never guest values.

use super::*;
use crate::alu::OperandUpdate;
use crate::test_step::{CallPatches, Engine};

fn write_then_read() -> TestModule {
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
                let first = body.parameter::<I32>(0)?;
                let second = body.parameter::<I32>(1)?;
                let mut accesses = memory.accesses();
                let write =
                    accesses.resolve(&mut body, &first, 8, Intent::Write, exit::exception)?;
                memory.write::<I64>(&mut body, &write, 0, &0x8877_6655_4433_2211u64.into())?;
                let read =
                    accesses.resolve(&mut body, &second, 4, Intent::Read, exit::exception)?;
                let value = memory.read::<I32>(&mut body, &read, 0)?;
                body.return_(value.unsigned().extend::<I64>())
            },
        )
        .unwrap();
    program.export("write_then_read", function).unwrap();
    TestModule::new(&crate::CompiledModule {
        execution_profile: None,
        bytes: program.compile().unwrap(),
        entry: "write_then_read".into(),
    })
}

fn check_sequence(
    engine: Engine,
    module: &TestModule,
    input: &Input,
    result: u64,
    changes: Vec<(u32, u8)>,
) {
    assert_eq!(
        engine.observe(module, input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(vec![Argument::I64(result as i64)]),
                snapshot: Snapshot {
                    cpu: input.cpu.clone(),
                    guest: Some(changes.clone()),
                },
            }],
            guest_unchanged: changes.is_empty(),
            machine_unchanged: true,
        },
        "{} at {:?}",
        module.entry,
        input.arguments,
    );
}

fn consecutive_accesses(engine: Engine) {
    let module = write_then_read();
    // Distinct linear pages alias the same frame. The later read must observe
    // the write, including when the first span has scattered or wrapping backing.
    for (first, second, expected, changes) in [
        (0x4000u32, 0x4004u32, 0x8877_6655u64, vec![(0x8000, 8)]),
        (0x4004, 0x4000, 0, vec![(0x8004, 8)]),
        (0x4ff8, 0x4ffc, 0x8877_6655, vec![(0x8ff8, 8)]),
        (0x4ffc, 0x4ffd, 0x5544_3322, vec![(0x8ffc, 4), (0xa000, 4)]),
        (0x4000, 0x6000, 0x4433_2211, vec![(0x8000, 8)]),
        (0x4ffc, 0x4000, 0, vec![(0x8ffc, 4), (0xa000, 4)]),
        (0xffff_fffc, 0, 0x8877_6655, vec![(0x8ffc, 4), (0xa000, 4)]),
        (0x4000, 0x7000, 0x0004_0000_0000_7000, vec![(0x8000, 8)]),
        (0x4000, 0x6ffd, 0x0004_0000_0000_7000, vec![(0x8000, 8)]),
    ] {
        let input = Input {
            machine: vec![
                (16, 0x8003u32.to_le_bytes().to_vec()),
                (20, 0xa003u32.to_le_bytes().to_vec()),
                (24, 0x8001u32.to_le_bytes().to_vec()),
                (0x003f_fffc, 0x8003u32.to_le_bytes().to_vec()),
                (0, 0xa003u32.to_le_bytes().to_vec()),
            ],
            arguments: vec![Argument::I32(first as i32), Argument::I32(second as i32)],
            observe_guest: true,
            ..Input::new(&CpuState::filled(0xa5).to_bytes())
        };
        let mut payload = 0x8877_6655_4433_2211u64.to_le_bytes().into_iter();
        let changes = changes
            .into_iter()
            .flat_map(|(address, bytes)| {
                (address..address + bytes)
                    .map(|address| (address, payload.next().unwrap()))
                    .collect::<Vec<_>>()
            })
            .collect();
        check_sequence(engine, &module, &input, expected, changes);
    }
}

fn same_address_updates(engine: Engine) {
    for first_write in [false, true] {
        let mut program = Program::new();
        let memory = Memory::declare(
            &mut program,
            crate::ExecutionProfile::Protected(crate::SegmentProfile::Flat32),
        )
        .unwrap();
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let address = body.parameter::<I32>(0)?;
                    let mut accesses = memory.accesses();
                    let intent = if first_write {
                        Intent::Write
                    } else {
                        Intent::Read
                    };
                    accesses.resolve(&mut body, &address, 8, intent, exit::exception)?;
                    let target =
                        accesses.resolve(&mut body, &address, 4, Intent::Write, exit::exception)?;
                    let previous = memory.atomic_update(
                        &mut body,
                        &target,
                        &OperandUpdate::<I32>::Exchange(0x1122_3344.into()),
                    )?;
                    body.return_(previous.unsigned().extend::<I64>())
                },
            )
            .unwrap();
        program.export("update", function).unwrap();
        let module = TestModule::new(&crate::CompiledModule {
            execution_profile: None,
            bytes: program.compile().unwrap(),
            entry: "update".into(),
        });
        for (address, first_permissions, second_permissions, result, changes) in [
            (
                0x4000u32,
                3u32,
                3u32,
                0u64,
                vec![
                    (0x8000, 0x44),
                    (0x8001, 0x33),
                    (0x8002, 0x22),
                    (0x8003, 0x11),
                ],
            ),
            (
                0x4ffc,
                3,
                3,
                0,
                vec![
                    (0x8ffc, 0x44),
                    (0x8ffd, 0x33),
                    (0x8ffe, 0x22),
                    (0x8fff, 0x11),
                ],
            ),
            (
                0x4ffe,
                3,
                3,
                0,
                vec![
                    (0x8ffe, 0x44),
                    (0x8fff, 0x33),
                    (0xa000, 0x22),
                    (0xa001, 0x11),
                ],
            ),
            (0x4000, 1, 3, 0x0004_0003_0000_4000, vec![]),
            (0x4ffe, 3, 1, 0x0004_0003_0000_5000, vec![]),
        ] {
            let input = Input {
                machine: vec![
                    (16, (0x8000 | first_permissions).to_le_bytes().to_vec()),
                    (20, (0xa000 | second_permissions).to_le_bytes().to_vec()),
                ],
                arguments: vec![Argument::I32(address as i32)],
                observe_guest: true,
                ..Input::new(&CpuState::filled(0xa5).to_bytes())
            };
            check_sequence(engine, &module, &input, result, changes);
        }
    }
}

fn page_changes_and_reentry(engine: Engine) {
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
                let first = body.parameter::<I32>(0)?;
                let second = body.parameter::<I32>(1)?;
                let mut accesses = memory.accesses();
                let target =
                    accesses.resolve(&mut body, &first, 4, Intent::Write, exit::exception)?;
                memory.write::<I32>(&mut body, &target, 0, &0x11.into())?;
                let mut sum = body.value::<I32>(0)?;
                for address in [second.clone(), second.add(4)] {
                    let access =
                        accesses.resolve(&mut body, &address, 4, Intent::Read, exit::exception)?;
                    sum = sum.add(memory.read::<I32>(&mut body, &access, 0)?);
                }
                body.return_(sum.unsigned().extend::<I64>())
            },
        )
        .unwrap();
    program.export("sum", function).unwrap();
    let module = TestModule::new(&crate::CompiledModule {
        execution_profile: None,
        bytes: program.compile().unwrap(),
        entry: "sum".into(),
    });
    let input = Input {
        guest: [
            (0x8000, 0x11u32),
            (0xa000, 0x22),
            (0xa004, 0x44),
            (0xb000, 0x11),
            (0xc000, 0x66),
            (0xc004, 0x88),
        ]
        .into_iter()
        .map(|(address, value)| (address, value.to_le_bytes().to_vec()))
        .collect(),
        machine: vec![
            (16, 0x8003u32.to_le_bytes().to_vec()),
            (20, 0xa001u32.to_le_bytes().to_vec()),
        ],
        arguments: vec![Argument::I32(0x4000), Argument::I32(0x5000)],
        observe_guest: true,
        patches_before_calls: vec![
            CallPatches::default(),
            CallPatches {
                machine: vec![
                    (16, 0xb003u32.to_le_bytes().to_vec()),
                    (20, 0xc001u32.to_le_bytes().to_vec()),
                ],
                ..CallPatches::default()
            },
            CallPatches {
                machine: vec![(20, 0u32.to_le_bytes().to_vec())],
                ..CallPatches::default()
            },
        ],
        ..Input::new(&CpuState::filled(0xa5).to_bytes())
    };
    assert_eq!(
        engine.observe(&module, &input, 3),
        Observation {
            events: [0x66u64, 0xee, 0x0004_0000_0000_5000]
                .into_iter()
                .map(|result| Event::Return {
                    outcome: Outcome::Returned(vec![Argument::I64(result as i64)]),
                    snapshot: Snapshot {
                        cpu: input.cpu.clone(),
                        guest: Some(vec![])
                    },
                })
                .collect(),
            guest_unchanged: true,
            // The observer includes the deliberately authored host remappings.
            machine_unchanged: false,
        }
    );
}

#[test]
fn consecutive_spans_preserve_aliases_boundaries_and_fault_progress() {
    consecutive_accesses(Engine::Wasmtime);
}

#[test]
fn narrower_updates_keep_their_width_and_require_write_permissions() {
    same_address_updates(Engine::Wasmtime);
}

#[test]
fn later_pages_and_new_entries_use_current_mappings() {
    page_changes_and_reentry(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_consecutive_spans_preserve_aliases_boundaries_and_fault_progress() {
    consecutive_accesses(Engine::V8);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_narrower_updates_keep_their_width_and_require_write_permissions() {
    same_address_updates(Engine::V8);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_later_pages_and_new_entries_use_current_mappings() {
    page_changes_and_reentry(Engine::V8);
}
