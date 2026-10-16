//! Vector transfers retain both halves at every page split and check the full span first.

use super::*;
use crate::test_step::Engine;
use wasm86_compiler::V128;

const PAYLOAD: u128 = 0xf0e0_d0c0_b0a0_9080_7060_5040_3020_1000;

fn module() -> TestModule {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program, crate::SegmentProfile::Flat32.into()).unwrap();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I64],
            },
            |mut body| {
                let start = body.parameter::<I32>(0)?;
                let access = memory.resolve_access(
                    &mut body,
                    &start,
                    16,
                    Intent::Write,
                    None,
                    Some(&mut exit::exception),
                )?;
                let before = memory.read::<V128>(&mut body, &access, 0)?;
                memory.write::<V128>(&mut body, &access, 0, &(!PAYLOAD).into())?;
                let after = memory.read::<V128>(&mut body, &access, 0)?;
                body.return_(
                    before
                        .eq_lanes::<I64>(PAYLOAD)
                        .bitmask::<I32>()
                        .eq(15)
                        .and(after.eq_lanes::<I64>(!PAYLOAD).bitmask::<I32>().eq(15))
                        .unsigned()
                        .extend::<I64>(),
                )
            },
        )
        .unwrap();
    program.export("replace", function).unwrap();
    TestModule::new(&crate::CompiledModule {
        execution_profile: None,
        bytes: program.compile().unwrap(),
        entry: "replace".into(),
    })
}

fn splits(engine: Engine) {
    let module = module();
    let payload = PAYLOAD.to_le_bytes();
    for first_page in [4_u32, 0xfffff] {
        for first_bytes in 1..16 {
            let start = (first_page << 12) | (4096 - first_bytes);
            let next_page = first_page.wrapping_add(1) & 0xfffff;
            for next_frame in [0x9000_u32, 0xa000] {
                for permissions in [3_u32, 1, 0] {
                    let input = Input {
                        machine: vec![
                            (first_page * 4, 0x8003_u32.to_le_bytes().to_vec()),
                            (
                                next_page * 4,
                                (next_frame | permissions).to_le_bytes().to_vec(),
                            ),
                        ],
                        guest: vec![
                            (
                                0x9000 - first_bytes,
                                payload[..first_bytes as usize].to_vec(),
                            ),
                            (next_frame, payload[first_bytes as usize..].to_vec()),
                        ],
                        arguments: vec![Argument::I32(start as i32)],
                        observe_guest: true,
                        ..Input::new(&CpuState::filled(0xa5).to_bytes())
                    };
                    let changes = if permissions == 3 {
                        payload
                            .iter()
                            .enumerate()
                            .map(|(index, byte)| {
                                let index = index as u32;
                                (
                                    if index < first_bytes {
                                        0x9000 - first_bytes + index
                                    } else {
                                        next_frame + index - first_bytes
                                    },
                                    !byte,
                                )
                            })
                            .collect()
                    } else {
                        vec![]
                    };
                    let result = if permissions == 3 {
                        1
                    } else {
                        0x0004_0000_0000_0000_u64
                            | (u64::from(2 | permissions) << 32)
                            | u64::from(next_page << 12)
                    };
                    assert_eq!(
                        engine.observe(&module, &input, 1),
                        Observation {
                            events: vec![Event::Return {
                                outcome: Outcome::Returned(vec![Argument::I64(result as i64)]),
                                snapshot: Snapshot {
                                    cpu: input.cpu.clone(),
                                    guest: Some(changes)
                                },
                            }],
                            guest_unchanged: permissions != 3,
                            machine_unchanged: true,
                        },
                        "start {start:x}, frame {next_frame:x}, permissions {permissions}"
                    );
                }
            }
        }
    }
}

#[test]
fn vector_transfers_cover_every_page_split_and_fault_before_writing() {
    splits(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_vector_transfers_cover_every_page_split_and_fault_before_writing() {
    splits(Engine::V8);
}
