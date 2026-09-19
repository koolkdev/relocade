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
