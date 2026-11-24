//! Entry capture, exact fetch and decoder continuations share the live CS default.

use super::{interpreter, step};
use crate::{
    support::machine::{check, Exit, Image, Step},
    test_step::{returned, CallPatches, Event, TestModule},
    CpuState,
    SegmentDefaultSize::{Bits16, Bits32},
};
use std::{mem::offset_of, sync::LazyLock};

fn run() -> &'static TestModule {
    static MODULE: LazyLock<TestModule> =
        LazyLock::new(|| TestModule::from_bytes(interpreter().bytes(), crate::abi::RUN.name, None));
    &MODULE
}

#[test]
fn exact_fetch_reads_only_the_live_immediate_width() {
    let mut image = Image::empty();
    image.cpu = CpuState::protected_mode(Bits16, Bits32);
    image.cpu.eip = 0x1ffd;
    image.cpu.registers.eax = 0xaaaa_aaaa;
    image.map(1, 0x3000, false);
    image.data(0x3ffd, &[0xb8, 0x34, 0x12]);
    let mut expected = image.cpu;
    expected.registers.eax = 0xaaaa_1234;
    expected.eip = 0x2000;
    expected.instruction_count += 1;
    assert_eq!(step(&image), expected);

    image.cpu.segments.cs.big = 1;
    image.check_unchanged_exit(
        interpreter(),
        "dword immediate reaches the absent next page",
        Exit::PageFault {
            address: 0x2000,
            error: 0x10,
        },
    );
}

#[test]
fn exact_fetch_reads_sib_only_for_32bit_addressing() {
    let mut image = Image::empty();
    image.cpu = CpuState::protected_mode(Bits16, Bits32);
    image.cpu.eip = 0x1ffe;
    image.cpu.registers.eax = 0xaaaa_aaaa;
    image.cpu.registers.esi = 0x7000;
    image.map(1, 0x3000, false);
    image.map(7, 0x5000, false);
    image.data(0x3ffe, &[0x8b, 0x04]); // MOV AX, [SI] has no SIB byte.
    image.data(0x5000, &[0x11, 0x22]);
    let mut expected = image.cpu;
    expected.registers.eax = 0xaaaa_2211;
    expected.eip = 0x2000;
    expected.instruction_count += 1;
    assert_eq!(step(&image), expected);

    image.cpu.segments.cs.big = 1;
    image.check_unchanged_exit(
        interpreter(),
        "SIB byte reaches the absent next page",
        Exit::PageFault {
            address: 0x2000,
            error: 0x10,
        },
    );
}

#[test]
fn prefixes_cross_pages_and_reset_after_memory_continuation() {
    for (default, expected_ecx, immediate, expected_eax) in [
        (Bits16, 0x8877_6655, &[0x78, 0x56][..], 0x5678),
        (
            Bits32,
            0xaaaa_2211,
            &[0x78, 0x56, 0x34, 0x12][..],
            0x1234_5678,
        ),
    ] {
        let mut image = Image::empty();
        image.cpu = CpuState::protected_mode(default, Bits32);
        image.cpu.eip = 0x1ffe;
        image.cpu.registers.eax = 0x7000;
        image.cpu.registers.ebx = 0x4000;
        image.cpu.registers.esi = 0x10;
        image.cpu.registers.ecx = 0xaaaa_aaaa;
        image.map(1, 0x3000, false);
        image.map(2, 0x6000, false);
        image.map(4, 0x4000, false);
        image.map(7, 0x5000, false);
        image.data(0x3ffe, &[0x66, 0x67]);
        image.data(0x6000, &[&[0x8b, 0x08, 0xb8], immediate, &[0xf1]].concat());
        image.data(0x4010, &[0x11, 0x22, 0x33, 0x44]);
        image.data(0x5000, &[0x55, 0x66, 0x77, 0x88]);
        let mut cpu = image.cpu;
        cpu.registers.ecx = expected_ecx;
        cpu.registers.eax = expected_eax;
        cpu.eip = 0x2003 + immediate.len() as u32;
        cpu.instruction_count += 2;
        check(
            run(),
            "unprefixed successor uses the entry default",
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::UnsupportedInstruction { eip: cpu.eip },
            }],
        );
    }
}

#[test]
fn reentry_recaptures_cs_default_in_the_same_instance() {
    let mut image = Image::new(&[0xb8, 0x34, 0x12, 0x78, 0x56]);
    image.cpu = CpuState::protected_mode(Bits16, Bits32);
    image.cpu.eip = 0x1000;
    image.cpu.registers.eax = 0xaaaa_aaaa;
    let mut input = image.input();
    input.patches_before_calls = vec![
        CallPatches::default(),
        CallPatches::cpu(vec![
            (offset_of!(CpuState, segments.cs.big) as u32, vec![1]),
            (
                offset_of!(CpuState, eip) as u32,
                0x1000u32.to_le_bytes().to_vec(),
            ),
        ]),
    ];
    let observation = interpreter().observe(&input, 2);
    assert!(observation.guest_unchanged && observation.machine_unchanged);
    assert_eq!(observation.events.len(), 2);
    for (event, (big, eax, eip, count)) in observation
        .events
        .iter()
        .zip([(0, 0xaaaa_1234, 0x1003, 1), (1, 0x5678_1234, 0x1005, 2)])
    {
        let Event::Return { outcome, snapshot } = event else {
            panic!("unexpected {event:?}")
        };
        assert_eq!(*outcome, returned(crate::ExecutionExit::StepComplete));
        let mut expected = image.cpu;
        expected.segments.cs.big = big;
        expected.registers.eax = eax;
        expected.eip = eip;
        expected.instruction_count = count;
        assert_eq!(snapshot.cpu, expected.to_bytes());
    }
}

#[test]
fn cs_reload_changes_fetch_base_and_default_for_the_next_entry() {
    use crate::{
        test_step::SegmentResolution, DescriptorTables, Segment, SegmentDescriptor,
        SegmentDescriptorKind, SegmentLimit,
    };
    let mut image = Image::new(&[0xea, 0x00, 0x02, 0, 0, 0x27, 0]);
    image.cpu = CpuState::protected_mode(Bits32, Bits32);
    image.cpu.eip = 0x1000;
    image.cpu.registers.eax = 0xaaaa_aaaa;
    image.map(9, 0x8000, false);
    image.data(0x8200, &[0xb8, 0x34, 0x12, 0x78, 0x56]);
    let mut tables = DescriptorTables::default();
    tables.insert(
        0x27,
        SegmentDescriptor::new(
            0x9000,
            SegmentLimit::from_effective(0xffff).unwrap(),
            SegmentDescriptorKind::Code {
                readable: true,
                conforming: false,
            },
            Bits16,
        ),
    );
    let mut input = image.input();
    input
        .segment_resolutions
        .push(SegmentResolution::new(&tables, Segment::Cs, 0x27));
    let observation = interpreter().observe(&input, 2);
    let [Event::ResolveSegment { segment, selector }, first, second] =
        observation.events.as_slice()
    else {
        panic!("unexpected events: {:?}", observation.events);
    };
    assert_eq!((*segment, *selector), (Segment::Cs as i32, 0x27));
    assert!(observation.guest_unchanged && observation.machine_unchanged);
    let mut expected = image.cpu;
    expected.segments.cs.selector = 0x27;
    expected.segments.cs.base = 0x9000;
    expected.segments.cs.limit = 0xffff;
    expected.segments.cs.big = 0;
    for (event, eax, eip, count) in [
        (first, 0xaaaa_aaaa, 0x200, 1),
        (second, 0xaaaa_1234, 0x203, 2),
    ] {
        let Event::Return { outcome, snapshot } = event else {
            panic!("unexpected {event:?}")
        };
        assert_eq!(*outcome, returned(crate::ExecutionExit::StepComplete));
        expected.registers.eax = eax;
        expected.eip = eip;
        expected.instruction_count = count;
        assert_eq!(snapshot.cpu, expected.to_bytes());
    }
}
