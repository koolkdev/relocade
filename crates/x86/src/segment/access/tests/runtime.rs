use super::*;
use crate::{
    test_step::{Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule},
    CpuState, SegmentAttributes, SegmentDefaultSize, SegmentKind, StoredSegment,
};

fn expect_address(
    engine: Engine,
    module: &TestModule,
    cpu: CpuState,
    choice: i32,
    address_or_fault: i64,
) {
    let bytes = cpu.to_bytes().to_vec();
    let input = Input {
        arguments: vec![Argument::I32(choice), Argument::I32(0x20)],
        ..Input::new(&bytes)
    };
    assert_eq!(
        engine.observe(module, &input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: Outcome::Returned(vec![Argument::I64(address_or_fault)]),
                snapshot: Snapshot {
                    cpu: bytes,
                    guest: None,
                },
            }],
            guest_unchanged: true,
            machine_unchanged: true,
        }
    );
}

fn check_segment_indices(engine: Engine) {
    let mut cpu = CpuState::default();
    cpu.segments.fs = StoredSegment {
        base: 0x8000,
        limit: 0x23,
        ..StoredSegment::flat_data32(0x53)
    };
    cpu.segments.gs = StoredSegment::unusable(0x63);
    for intent in [Intent::Read, Intent::Write] {
        let module = TestModule::new(&translation(
            SegmentProfile::Flat32,
            intent,
            Some(SegmentSelection::Indexed),
        ));
        for (choice, expected) in [
            (0, 0x20),
            (
                1,
                if matches!(intent, Intent::Write) {
                    2 << 48
                } else {
                    0x20
                },
            ),
            (2, 0x20),
            (3, 0x20),
            (4, 0x8020),
            (5, 2 << 48),
        ] {
            expect_address(engine, &module, cpu, choice, expected);
        }
    }
    let read = TestModule::new(&translation(
        SegmentProfile::Flat32,
        Intent::Read,
        Some(SegmentSelection::Indexed),
    ));
    cpu.segments.cs.attributes = SegmentAttributes::new(
        SegmentKind::Code { readable: false },
        SegmentDefaultSize::Bits32,
    );
    let segmented_read = TestModule::new(&translation(
        SegmentProfile::Segmented32,
        Intent::Read,
        Some(SegmentSelection::Indexed),
    ));
    expect_address(engine, &segmented_read, cpu, 1, 2 << 48);
    cpu.segments.cs = StoredSegment::flat_code32(0);
    cpu.segments.fs.limit = 0x22;
    expect_address(engine, &read, cpu, 4, 2 << 48);

    let address_default = TestModule::new(&translation(
        SegmentProfile::Segmented32,
        Intent::Read,
        Some(default_segment),
    ));
    cpu.segments.ds.base = 0x4000;
    cpu.segments.ss.base = 0x8000;
    expect_address(engine, &address_default, cpu, 0, 0x8020);
    expect_address(engine, &address_default, cpu, 1, 0x4020);
    cpu.segments.ss.limit = 0x22;
    expect_address(engine, &address_default, cpu, 0, 16 << 48);
    cpu.segments.ds.limit = 0x22;
    expect_address(engine, &address_default, cpu, 1, 2 << 48);
}

#[test]
fn runtime_overrides_and_defaults_preserve_translation_and_faults() {
    check_segment_indices(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_runtime_overrides_and_defaults_preserve_translation_and_faults() {
    check_segment_indices(Engine::V8);
}
