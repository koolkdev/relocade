use super::*;
use crate::test_step::Engine;

fn field_module() -> TestModule {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let start = body.parameter::<I32>(0)?;
                let access =
                    memory.resolve_access(&mut body, &start, 6, Intent::Write, exit::exception)?;
                let word = memory.read::<I16>(&mut body, &access, 0)?;
                let dword = memory.read::<I32>(&mut body, &access, 2)?;
                let byte = memory.read::<I8>(&mut body, &access, 5)?;
                memory.write::<I8>(&mut body, &access, 5, &0x7au32.into())?;
                let replaced = memory.read::<I8>(&mut body, &access, 5)?;
                body.return_(
                    word.eq(0x2211)
                        .and(dword.eq(0x6655_4433))
                        .and(byte.eq(0x66))
                        .and(replaced.eq(0x7a))
                        .unsigned()
                        .extend::<I64>(),
                )
            },
        )
        .unwrap();
    program.export("fields", function).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    TestModule::new(&crate::CompiledModule {
        segment_profile: None,
        bytes,
        entry: "fields".into(),
    })
}

fn fields(engine: Engine) {
    let module = field_module();
    for linear in [
        0x4000u32,
        0x4001,
        0x4ffa,
        0x4ffc,
        0x4ffe,
        0x4fff,
        0xffff_fffc,
        0xffff_ffff,
    ] {
        for next_frame in [0x9000, 0xa000] {
            let split = (0x1000 - (linear & 0xfff)).min(6) as usize;
            let payload = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
            let physical = 0x8000 + (linear & 0xfff);
            let mut input = Input {
                guest: vec![(physical, payload[..split].to_vec())],
                machine: vec![((linear >> 12) * 4, 0x8003u32.to_le_bytes().to_vec())],
                arguments: vec![Argument::I32(linear as i32)],
                observe_guest: true,
                ..Input::new(&CpuState::filled(0xa5).to_bytes())
            };
            if split < 6 {
                input.machine.push((
                    (linear.wrapping_add(6) >> 12) * 4,
                    (next_frame | 3u32).to_le_bytes().to_vec(),
                ));
                input.guest.push((next_frame, payload[split..].to_vec()));
            }
            let changed = if split < 6 {
                next_frame + (5 - split as u32)
            } else {
                physical + 5
            };
            assert_eq!(
                engine.observe(&module, &input, 1),
                Observation {
                    events: vec![Event::Return {
                        outcome: Outcome::Returned(vec![Argument::I64(1)]),
                        snapshot: Snapshot {
                            cpu: CpuState::filled(0xa5).to_bytes().to_vec(),
                            guest: Some(vec![(changed, 0x7a)])
                        },
                    }],
                    guest_unchanged: false,
                    machine_unchanged: true,
                },
                "start {linear:x}, second frame {next_frame:x}"
            );
        }
    }
}

#[test]
fn checked_spans_locate_mixed_width_fields_across_frames_and_linear_wrap() {
    fields(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_checked_spans_with_mixed_width_fields() {
    fields(Engine::V8);
}

const WIDE_FIELDS: [u8; 10] = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa];
const REPLACEMENT_FIELDS: [u8; 10] = [0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55];

fn wide_field_modules() -> [TestModule; 2] {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    for (name, intent) in [
        ("read_fields", Intent::Read),
        ("write_fields", Intent::Write),
    ] {
        let function = program
            .function(
                Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I64],
                },
                |mut body| {
                    let start = body.parameter::<I32>(0)?;
                    let access =
                        memory.resolve_access(&mut body, &start, 10, intent, exit::exception)?;
                    match intent {
                        Intent::Read => {
                            let qword = memory.read::<I64>(&mut body, &access, 0)?;
                            let word = memory.read::<I16>(&mut body, &access, 8)?;
                            body.return_(
                                qword
                                    .eq(0x8877_6655_4433_2211u64)
                                    .and(word.eq(0xaa99))
                                    .unsigned()
                                    .extend::<I64>(),
                            )
                        }
                        Intent::Write => {
                            memory.write::<I64>(
                                &mut body,
                                &access,
                                0,
                                &0x7788_99aa_bbcc_ddeeu64.into(),
                            )?;
                            memory.write::<I16>(&mut body, &access, 8, &0x5566u32.into())?;
                            body.return_(1)
                        }
                        Intent::Fetch => unreachable!(),
                    }
                },
            )
            .unwrap();
        program.export(name, function).unwrap();
    }
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    ["read_fields", "write_fields"].map(|entry| {
        TestModule::new(&crate::CompiledModule {
            segment_profile: None,
            bytes: bytes.clone(),
            entry: entry.into(),
        })
    })
}

fn wide_input(linear: u32, next_frame: u32, permissions: [u32; 2]) -> Input {
    let split = (0x1000 - (linear & 0xfff)).min(10) as usize;
    let mut input = Input {
        guest: vec![(0x8000 + (linear & 0xfff), WIDE_FIELDS[..split].to_vec())],
        machine: vec![(
            (linear >> 12) * 4,
            (0x8000 | permissions[0]).to_le_bytes().to_vec(),
        )],
        arguments: vec![Argument::I32(linear as i32)],
        observe_guest: true,
        ..Input::new(&CpuState::filled(0xa5).to_bytes())
    };
    if split < 10 {
        input.machine.push((
            (linear.wrapping_add(9) >> 12) * 4,
            (next_frame | permissions[1]).to_le_bytes().to_vec(),
        ));
        input
            .guest
            .push((next_frame, WIDE_FIELDS[split..].to_vec()));
    }
    input
}

fn check_wide_fields(
    engine: Engine,
    module: &TestModule,
    input: &Input,
    result: u64,
    changes: Vec<(u32, u8)>,
) {
    let guest_unchanged = changes.is_empty();
    assert_eq!(
        engine.observe(module, input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(vec![Argument::I64(result as i64)]),
                snapshot: Snapshot {
                    cpu: CpuState::filled(0xa5).to_bytes().to_vec(),
                    guest: Some(changes),
                },
            }],
            guest_unchanged,
            machine_unchanged: true,
        },
        "{} at {:?}",
        module.entry,
        input.arguments,
    );
}

fn wide_fields(engine: Engine) {
    let [read, write] = wide_field_modules();
    let starts = [0x4000u32, 0x4001, 0x4ff6]
        .into_iter()
        .chain(0x4ff7..=0x4fff)
        .chain([0xffff_fff7, 0xffff_fff8, 0xffff_ffff]);
    for linear in starts {
        for next_frame in [0x9000, 0xa000] {
            let input = wide_input(linear, next_frame, [3, 3]);
            check_wide_fields(engine, &read, &input, 1, Vec::new());
            let split = (0x1000 - (linear & 0xfff)).min(10);
            let changes = REPLACEMENT_FIELDS
                .into_iter()
                .enumerate()
                .map(|(index, byte)| {
                    let index = index as u32;
                    let physical = if index < split {
                        0x8000 + (linear & 0xfff) + index
                    } else {
                        next_frame + index - split
                    };
                    (physical, byte)
                })
                .collect();
            check_wide_fields(engine, &write, &input, 1, changes);
        }
    }
    // The final word can occupy a different page even when the qword fits in
    // the first. Every denial must leave both fields unchanged.
    for linear in [0x4ff7u32, 0x4ff8, 0x4fff, 0xffff_fff8, 0xffff_ffff] {
        let next_page = linear.wrapping_add(9) & !0xfff;
        for (permissions, write_access, address, error_code) in [
            ([0, 3], false, linear, 0),
            ([3, 0], false, next_page, 0),
            ([0, 3], true, linear, 2),
            ([1, 3], true, linear, 3),
            ([3, 0], true, next_page, 2),
            ([3, 1], true, next_page, 3),
        ] {
            let input = wide_input(linear, 0xa000, permissions);
            let result = 0x0004_0000_0000_0000 | (error_code << 32) | u64::from(address);
            check_wide_fields(
                engine,
                if write_access { &write } else { &read },
                &input,
                result,
                Vec::new(),
            );
        }
    }
}

#[test]
fn ten_byte_spans_transfer_qword_and_word_fields_after_complete_checks() {
    wide_fields(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_ten_byte_spans_transfer_qword_and_word_fields_after_complete_checks() {
    wide_fields(Engine::V8);
}
