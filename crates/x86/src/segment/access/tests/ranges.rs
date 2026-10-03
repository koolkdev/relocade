use super::*;
use crate::{
    test_step::{Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule},
    CpuState, SegmentAttributes, SegmentDefaultSize, SegmentKind, StoredSegment,
};

fn check_dynamic_spans(engine: Engine) {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let access = SegmentAccess::new(&cpu, SegmentProfile::Segmented32);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I32, Type::I32],
            },
            |mut body| {
                let offset = body.parameter::<I32>(0)?;
                let bytes = body.parameter::<I32>(1)?;
                let check = access.check(
                    &mut body,
                    &Segment::Fs.into(),
                    &offset,
                    bytes,
                    Intent::Write,
                )?;
                body.return_((
                    check.linear,
                    check.denied.unwrap().unsigned().extend::<I32>(),
                ))
            },
        )
        .unwrap();
    program.export("probe", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "probe".into(),
        segment_profile: Some(SegmentProfile::Segmented32),
    });
    for (down, offset, bytes, denied) in [
        (false, 0x20u32, 4, false),
        (false, 0x20, 5, true),
        (false, u32::MAX - 1, 4, true),
        (true, 0x23, 1, true),
        (true, 0x24, 1, false),
        (true, 0xfffc, 4, false),
        (true, 0xfffc, 5, true),
        (true, u32::MAX - 1, 4, true),
    ] {
        let mut cpu = CpuState::default();
        cpu.segments.fs = StoredSegment {
            base: 0x8000,
            limit: 0x23,
            attributes: SegmentAttributes::new(
                SegmentKind::Data {
                    writable: true,
                    expand_down: down,
                },
                SegmentDefaultSize::Bits16,
            ),
            ..StoredSegment::flat_data32(0x53)
        };
        let cpu = cpu.to_bytes().to_vec();
        let input = Input {
            arguments: vec![Argument::I32(offset as i32), Argument::I32(bytes)],
            ..Input::new(&cpu)
        };
        assert_eq!(
            engine.observe(&module, &input, 1),
            Observation {
                events: vec![Event::Return {
                    outcome: Outcome::Returned(vec![
                        Argument::I32(offset.wrapping_add(0x8000) as i32),
                        Argument::I32(i32::from(denied)),
                    ]),
                    snapshot: Snapshot { cpu, guest: None },
                }],
                guest_unchanged: true,
                machine_unchanged: true,
            }
        );
    }
}

#[test]
fn dynamic_segment_spans_check_limits_and_wrap() {
    check_dynamic_spans(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_dynamic_segment_spans_check_limits_and_wrap() {
    check_dynamic_spans(Engine::V8);
}
