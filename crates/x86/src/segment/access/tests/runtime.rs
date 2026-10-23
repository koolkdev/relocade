use super::*;
use crate::SegmentProfile;
use crate::{
    test_step::{returned, Argument, Event, Input, Observation, Snapshot, TestModule},
    CpuState, SegmentRegister,
};

fn expect_address(
    module: &TestModule,
    cpu: CpuState,
    choice: i32,
    address_or_fault: impl Into<AccessResult>,
) {
    let bytes = cpu.to_bytes().to_vec();
    let input = Input {
        arguments: vec![Argument::I32(choice), Argument::I32(0x20)],
        ..Input::new(&bytes)
    };
    assert_eq!(
        module.observe(&input, 1),
        Observation {
            events: vec![Event::Return {
                outcome: returned::<AccessResult>(address_or_fault.into()),
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

#[test]
fn runtime_overrides_and_defaults_preserve_translation_and_faults() {
    let mut cpu = CpuState::default();
    cpu.segments.fs = SegmentRegister {
        base: 0x8000,
        limit: 0x23,
        ..SegmentRegister::flat_data32(0x53)
    };
    cpu.segments.gs = SegmentRegister::unusable(0x63);
    for intent in [Intent::Read, Intent::Write] {
        let module = TestModule::new(&translation(
            ExecutionProfile::Protected(SegmentProfile::Flat32),
            intent,
            Some(SegmentSelection::indexed),
        ));
        for (choice, expected) in [
            (0, AccessResult::Value(0x20)),
            (
                1,
                if matches!(intent, Intent::Write) {
                    AccessResult::Fault(crate::Exception::GeneralProtection { error_code: 0 })
                } else {
                    AccessResult::Value(0x20)
                },
            ),
            (2, AccessResult::Value(0x20)),
            (3, AccessResult::Value(0x20)),
            (4, AccessResult::Value(0x8020)),
            (
                5,
                AccessResult::Fault(crate::Exception::GeneralProtection { error_code: 0 }),
            ),
        ] {
            expect_address(&module, cpu, choice, expected);
        }
    }
    let read = TestModule::new(&translation(
        ExecutionProfile::Protected(SegmentProfile::Flat32),
        Intent::Read,
        Some(SegmentSelection::indexed),
    ));
    cpu.segments.cs.big = 1;
    cpu.segments.cs.readable = 0;
    let segmented_read = TestModule::new(&translation(
        ExecutionProfile::Protected(SegmentProfile::Segmented32),
        Intent::Read,
        Some(SegmentSelection::indexed),
    ));
    expect_address(
        &segmented_read,
        cpu,
        1,
        AccessResult::Fault(crate::Exception::GeneralProtection { error_code: 0 }),
    );
    cpu.segments.cs = SegmentRegister::flat_code32(0);
    cpu.segments.fs.limit = 0x22;
    expect_address(
        &read,
        cpu,
        4,
        AccessResult::Fault(crate::Exception::GeneralProtection { error_code: 0 }),
    );

    let address_default = TestModule::new(&translation(
        ExecutionProfile::Protected(SegmentProfile::Segmented32),
        Intent::Read,
        Some(default_segment),
    ));
    cpu.segments.ds.base = 0x4000;
    cpu.segments.ss.base = 0x8000;
    expect_address(&address_default, cpu, 0, 0x8020);
    expect_address(&address_default, cpu, 1, 0x4020);
    cpu.segments.ss.limit = 0x22;
    expect_address(
        &address_default,
        cpu,
        0,
        AccessResult::Fault(crate::Exception::StackFault { error_code: 0 }),
    );
    cpu.segments.ds.limit = 0x22;
    expect_address(
        &address_default,
        cpu,
        1,
        AccessResult::Fault(crate::Exception::GeneralProtection { error_code: 0 }),
    );
}
