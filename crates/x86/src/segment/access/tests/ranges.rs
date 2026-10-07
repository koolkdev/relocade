use super::*;
use crate::{
    test_step::{Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule},
    CpuState, SegmentAttributes, SegmentDefaultSize, SegmentKind, StoredSegment,
};

fn probe(profile: crate::ExecutionProfile, segment: Segment) -> TestModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let access = SegmentAccess::new(&cpu, profile);
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I32, Type::I32],
            },
            |mut body| {
                let offset = body.parameter::<I32>(0)?;
                let bytes = body.parameter::<I32>(1)?;
                let check =
                    access.check(&mut body, &segment.into(), &offset, bytes, Intent::Write)?;
                body.return_((
                    check.linear,
                    check.denied.unwrap().unsigned().extend::<I32>(),
                ))
            },
        )
        .unwrap();
    program.export("probe", function).unwrap();
    TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "probe".into(),
        execution_profile: Some(profile),
    })
}

fn dynamic_spans(engine: Engine) {
    let module = probe(SegmentProfile::Segmented32.into(), Segment::Fs);
    use SegmentDefaultSize::{Bits16, Bits32};
    for (down, size, limit, cases) in [
        (
            false,
            Bits16,
            0x23,
            &[
                (0x20u32, 4u32, false),
                (0x20, 5, true),
                (u32::MAX - 1, 4, true),
            ][..],
        ),
        (
            false,
            Bits32,
            0x10000,
            &[(0xf000, 4097, false), (0xf000, 4098, true)],
        ),
        (
            false,
            Bits32,
            u32::MAX,
            &[(u32::MAX - 1, 4, false), (0x20, u32::MAX, false)],
        ),
        (
            true,
            Bits16,
            0x23,
            &[
                (0x23, 1, true),
                (0x24, 1, false),
                (0xfffc, 4, false),
                (0xfffc, 5, true),
                (u32::MAX - 1, 4, true),
            ],
        ),
        (
            true,
            Bits32,
            0x1000,
            &[(0x1001, 0xffff_efff, false), (0x1001, 0xffff_f000, true)],
        ),
    ] {
        let mut cpu = CpuState::default();
        cpu.segments.fs = StoredSegment {
            base: 0x8000,
            limit,
            attributes: SegmentAttributes::new(
                SegmentKind::Data {
                    writable: true,
                    expand_down: down,
                },
                size,
            ),
            ..StoredSegment::flat_data32(0x53)
        };
        let cpu = cpu.to_bytes().to_vec();
        for &(offset, bytes, denied) in cases {
            let input = Input {
                arguments: vec![Argument::I32(offset as i32), Argument::I32(bytes as i32)],
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
                        snapshot: Snapshot {
                            cpu: cpu.clone(),
                            guest: None
                        },
                    }],
                    guest_unchanged: true,
                    machine_unchanged: true,
                },
                "down {down}, limit {limit:x}, offset {offset:x}, bytes {bytes}"
            );
        }
    }
}

#[test]
fn runtime_segment_spans_check_limits_and_wrap() {
    dynamic_spans(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_runtime_segment_spans_check_limits_and_wrap() {
    dynamic_spans(Engine::V8);
}

fn real_mode_spans(engine: Engine) {
    let module = probe(crate::ExecutionProfile::Real16, Segment::Cs);
    let mut cpu = CpuState {
        segments: crate::Segments::real_mode(),
        ..CpuState::default()
    };
    cpu.segments.cs = StoredSegment::real_mode(Segment::Cs, 0x1234);
    let cpu = cpu.to_bytes().to_vec();
    for (offset, bytes, denied) in [
        (0u32, 0x10000u32, false),
        (1, 0x10000, true),
        (0, 0x10001, true),
        (0xfffc, 4, false),
        (0xfffc, 5, true),
        (0xffff, 1, false),
        (0x10000, 1, true),
        (0, u32::MAX, true),
        (1, u32::MAX, true),
        (u32::MAX, 2, true),
    ] {
        let input = Input {
            arguments: vec![Argument::I32(offset as i32), Argument::I32(bytes as i32)],
            ..Input::new(&cpu)
        };
        assert_eq!(
            engine.observe(&module, &input, 1),
            Observation {
                events: vec![Event::Return {
                    outcome: Outcome::Returned(vec![
                        Argument::I32(offset.wrapping_add(0x12340) as i32),
                        Argument::I32(i32::from(denied)),
                    ]),
                    snapshot: Snapshot {
                        cpu: cpu.clone(),
                        guest: None
                    },
                }],
                guest_unchanged: true,
                machine_unchanged: true,
            },
            "offset {offset:x}, bytes {bytes}"
        );
    }
}

#[test]
fn real_mode_runtime_spans_keep_the_64k_limit_and_allow_cs_writes() {
    real_mode_spans(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_real_mode_runtime_spans_keep_the_64k_limit_and_allow_cs_writes() {
    real_mode_spans(Engine::V8);
}
