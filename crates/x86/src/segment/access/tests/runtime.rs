use super::*;
use crate::support::config::{FLAT32, SEGMENTED32};

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
fn one_translation_reads_mode_and_segment_fields_from_each_cpu_image() {
    let module = TestModule::new(&translation(
        None,
        Intent::Write,
        Some(SegmentSelection::indexed),
    ));
    let mut cpu = CpuState::real_mode();
    cpu.segments.cs.base = 0x4000;
    cpu.segments.ss.base = 0x8000;
    cpu.segments.ss.writable = 0;
    let pe = crate::state::cr0::PROTECTION_ENABLE;
    for (protection, vm, protected) in [(0, 0, false), (pe, 0, true), (pe, 1, false)] {
        cpu.cr0 = (cpu.cr0 & !pe) | protection;
        cpu.flags.bytes.vm = vm;
        expect_address(
            &module,
            cpu,
            Segment::Cs as i32,
            if protected {
                AccessResult::Fault(crate::Exception::GeneralProtection { error_code: 0 })
            } else {
                AccessResult::Value(0x4020)
            },
        );
        expect_address(
            &module,
            cpu,
            Segment::Ss as i32,
            if protected {
                AccessResult::Fault(crate::Exception::StackFault { error_code: 0 })
            } else {
                AccessResult::Value(0x8020)
            },
        );
    }

    // The same loaded SS limit constrains both real and protected accesses.
    cpu.flags.bytes.vm = 0;
    cpu.segments.ss.writable = 1;
    cpu.segments.ss.limit = 0x22;
    for protection in [0, pe] {
        cpu.cr0 = (cpu.cr0 & !pe) | protection;
        expect_address(
            &module,
            cpu,
            Segment::Ss as i32,
            AccessResult::Fault(crate::Exception::StackFault { error_code: 0 }),
        );
    }
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
            FLAT32,
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
        FLAT32,
        Intent::Read,
        Some(SegmentSelection::indexed),
    ));
    cpu.segments.cs.big = 1;
    cpu.segments.cs.readable = 0;
    let segmented_read = TestModule::new(&translation(
        SEGMENTED32,
        Intent::Read,
        Some(SegmentSelection::indexed),
    ));
    expect_address(
        &segmented_read,
        cpu,
        1,
        AccessResult::Fault(crate::Exception::GeneralProtection { error_code: 0 }),
    );
    cpu.segments.cs = SegmentRegister::flat_code32(0x1b);
    cpu.segments.fs.limit = 0x22;
    expect_address(
        &read,
        cpu,
        4,
        AccessResult::Fault(crate::Exception::GeneralProtection { error_code: 0 }),
    );

    let address_default = TestModule::new(&translation(
        SEGMENTED32,
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
