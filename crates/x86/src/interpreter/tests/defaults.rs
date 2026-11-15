//! The same decoder reads operand defaults from each entering CPU image.

use crate::{
    support::machine::Image,
    test_step::{returned, Event, TestModule},
    CpuState, ExecutionExit, SegmentDefaultSize, SegmentProfile,
};
use std::sync::LazyLock;

fn interpreter() -> &'static TestModule {
    static MODULE: LazyLock<TestModule> = LazyLock::new(|| {
        let bytes = super::super::build(SegmentProfile::Segmented32.into(), None)
            .unwrap()
            .compile()
            .unwrap();
        // Only the decoder's CS default varies; execution keeps supported
        // protected-mode caches, privilege and memory routing.
        TestModule::from_bytes(&bytes, crate::abi::STEP.name, None)
    });
    &MODULE
}

fn step(initial: CpuState, code: &[u8]) -> CpuState {
    let mut image = Image::new(code);
    image.cpu = initial;
    let observation = interpreter().observe(&image.input(), 1);
    let [Event::Return { outcome, snapshot }] = observation.events.as_slice() else {
        panic!("unexpected decoder events: {:?}", observation.events);
    };
    assert_eq!(*outcome, returned(ExecutionExit::StepComplete));
    assert!(observation.guest_unchanged && observation.machine_unchanged);
    CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap())
}

#[test]
fn live_code_defaults_and_repeated_operand_overrides() {
    for (default, prefixes, expected_eax, length) in [
        (SegmentDefaultSize::Bits16, &[][..], 0xaaaa_1234, 3),
        (SegmentDefaultSize::Bits32, &[][..], 0x5678_1234, 5),
        (SegmentDefaultSize::Bits16, &[0x66][..], 0x5678_1234, 6),
        (SegmentDefaultSize::Bits32, &[0x66][..], 0xaaaa_1234, 4),
        (
            SegmentDefaultSize::Bits16,
            &[0x66, 0x66][..],
            0x5678_1234,
            7,
        ),
        (
            SegmentDefaultSize::Bits32,
            &[0x66, 0x66][..],
            0xaaaa_1234,
            5,
        ),
    ] {
        let mut initial = CpuState::protected_mode(default, SegmentDefaultSize::Bits32);
        initial.eip = 0x1000;
        initial.registers.eax = 0xaaaa_aaaa;
        let code = [prefixes, &[0xb8, 0x34, 0x12, 0x78, 0x56]].concat();
        let mut expected = initial;
        expected.registers.eax = expected_eax;
        expected.eip += length;
        expected.instruction_count += 1;
        assert_eq!(step(initial, &code), expected);
    }
}
