use super::*;
use crate::support::config::REAL16;
use crate::{
    test_step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule},
    CpuState, SegmentDefaultSize, SegmentRegister,
};

fn probe(segment: Segment) -> TestModule {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function =
        program.function::<fn(u32, u32) -> (u32, u32)>(move |mut body, (offset, bytes)| {
            let mut state = State::new(&cpu);
            let check = state.segments().check(
                &mut body,
                &segment.into(),
                &offset,
                bytes,
                Intent::Write,
            )?;
            body.return_((
                check.linear,
                check.denied.unwrap().unsigned().extend::<I32>(),
            ))
        });
    program.export("probe", function).unwrap();
    TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "probe".into(),
        config: None,
    })
}

#[test]
fn runtime_segment_spans_check_limits_and_wrap() {
    let module = probe(Segment::Fs);
    use SegmentDefaultSize::{Bits16, Bits32};
    for (down, size, limit, cases) in [
        (
            false,
            Bits16,
            0x23,
            &[
                (0x20u32, 0u32, true),
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
        cpu.segments.fs = SegmentRegister {
            base: 0x8000,
            limit,
            expand_down: u8::from(down),
            big: u8::from(size == Bits32),
            ..SegmentRegister::flat_data32(0x53)
        };
        let cpu = cpu.to_bytes().to_vec();
        for &(offset, bytes, denied) in cases {
            let input = Input {
                arguments: vec![Argument::I32(offset as i32), Argument::I32(bytes as i32)],
                ..Input::new(&cpu)
            };
            assert_eq!(
                module.observe(&input, 1),
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
fn real_mode_spans_use_the_loaded_limit_and_allow_cs_writes() {
    let module = probe(Segment::Cs);
    let mut cpu = CpuState::real_mode();
    cpu.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x1234);
    for (limit, offset, bytes, denied) in [
        (0xffff, 0u32, 0u32, true),
        (0xffff, 0, 0x10000, false),
        (0xffff, 1, 0x10000, true),
        (0xffff, 0, 0x10001, true),
        (0xffff, 0xfffc, 4, false),
        (0xffff, 0xfffc, 5, true),
        (0xffff, 0xffff, 1, false),
        (0xffff, 0x10000, 1, true),
        (0xffff, 0, u32::MAX, true),
        (0xffff, 1, u32::MAX, true),
        (0xffff, u32::MAX, 2, true),
        (0x23, 0x20, 4, false),
        (0x23, 0x20, 5, true),
        (0x1ffff, 0x10000, 0x10000, false),
        (0x1ffff, 0x10001, 0x10000, true),
        (u32::MAX, u32::MAX, 1, false),
        (u32::MAX, u32::MAX, 2, true),
    ] {
        cpu.segments.cs.limit = limit;
        let cpu = cpu.to_bytes().to_vec();
        let input = Input {
            arguments: vec![Argument::I32(offset as i32), Argument::I32(bytes as i32)],
            ..Input::new(&cpu)
        };
        assert_eq!(
            module.observe(&input, 1),
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
            "limit {limit:x}, offset {offset:x}, bytes {bytes}"
        );
    }
}

#[test]
fn constant_empty_span_is_denied_without_faulting() {
    let mut program = Program::new();
    let cpu = Cpu::declare(&mut program);
    let function = program.function::<fn() -> u32>(move |mut body, ()| {
        let mut state = State::for_entry(
            &cpu,
            &crate::profile::EntryGuarantees::for_entry(REAL16.entry),
        );
        let check =
            state
                .segments()
                .check(&mut body, &Segment::Ss.into(), &0.into(), 0, Intent::Read)?;
        body.return_(check.denied.unwrap().unsigned().extend::<I32>())
    });
    program.export("probe", function).unwrap();
    let module = TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "probe".into(),
        config: Some(REAL16),
    });
    let cpu = CpuState::real_mode().to_bytes().to_vec();
    assert_eq!(
        module.observe(&Input::new(&cpu), 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(vec![Argument::I32(1)]),
                snapshot: Snapshot { cpu, guest: None },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        }
    );
}
