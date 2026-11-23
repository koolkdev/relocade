//! The same decoder reads operand and address defaults from each entering CPU image.

mod entries;

use crate::{
    support::machine::Image,
    test_step::{returned, Event, TestModule},
    CpuState, ExecutionExit, SegmentDefaultSize, SegmentProfile,
};
use std::sync::LazyLock;

fn interpreter() -> &'static TestModule {
    static MODULE: LazyLock<TestModule> = LazyLock::new(|| {
        let profile = SegmentProfile::Segmented32.into();
        let guarantees = crate::profile::EntryGuarantees::new(profile, None);
        let bytes = super::super::build(profile, crate::MemoryBackend::HostPageTable, guarantees)
            .unwrap()
            .compile()
            .unwrap();
        // CS.D is live in decoding and execution. Other mode, privilege and
        // memory assumptions still describe supported protected-mode execution.
        TestModule::from_bytes(&bytes, crate::abi::STEP.name, None)
    });
    &MODULE
}

fn step(image: &Image) -> CpuState {
    let observation = interpreter().observe(&image.input(), 1);
    let [Event::Return { outcome, snapshot }] = observation.events.as_slice() else {
        panic!("unexpected decoder events: {:?}", observation.events);
    };
    assert_eq!(*outcome, returned(ExecutionExit::StepComplete));
    assert!(observation.guest_unchanged && observation.machine_unchanged);
    CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap())
}

#[test]
fn fxsave_and_fxrstor_use_the_entering_code_width_for_pointer_offsets() {
    use SegmentDefaultSize::{Bits16, Bits32};
    let offset = 0x1234_5678u32;
    for (default, expected) in [(Bits16, 0x5678u32), (Bits32, offset)] {
        for (modrm, restore) in [(0x07, false), (0x0f, true)] {
            // The same ModRM addresses [BX] at 16 bits and [EDI] at 32 bits.
            let mut image = Image::new(&[0x0f, 0xae, modrm]);
            image.cpu = CpuState::protected_mode(default, Bits32);
            image.cpu.eip = 0x1000;
            image.cpu.registers.ebx = 0x2000;
            image.cpu.registers.edi = 0x2000;
            image.map(2, 0x4000, true);
            if restore {
                image.data(0x4008, &offset.to_le_bytes());
                assert_eq!(step(&image).x87.instruction_offset, expected);
            } else {
                image.cpu.x87.instruction_offset = offset;
                // Every destination byte changes so the snapshot includes the whole offset.
                image.data(0x4008, &[0xff; 4]);
                let observation = interpreter().observe(&image.input(), 1);
                let [Event::Return { outcome, snapshot }] = observation.events.as_slice() else {
                    panic!("unexpected FXSAVE events: {:?}", observation.events);
                };
                assert_eq!(*outcome, returned(ExecutionExit::StepComplete));
                let saved =
                    snapshot
                        .guest
                        .as_ref()
                        .unwrap()
                        .iter()
                        .filter_map(|&(address, byte)| {
                            (0x4008..0x400c).contains(&address).then_some(byte)
                        });
                assert!(saved.eq(expected.to_le_bytes()));
            }
        }
    }
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
        let mut image = Image::new(&code);
        image.cpu = initial;
        assert_eq!(step(&image), expected);
    }
}

#[test]
fn live_address_defaults_and_independent_prefixes() {
    use SegmentDefaultSize::{Bits16, Bits32};
    for (default, prefixes, expected_ecx) in [
        (Bits16, &[][..], 0xaaaa_2211),
        (Bits32, &[][..], 0x8877_6655),
        (Bits16, &[0x67][..], 0xaaaa_6655),
        (Bits32, &[0x67][..], 0x4433_2211),
        (Bits16, &[0x66][..], 0x4433_2211),
        (Bits32, &[0x66][..], 0xaaaa_6655),
        (Bits16, &[0x66, 0x67][..], 0x8877_6655),
        (Bits32, &[0x66, 0x67][..], 0xaaaa_2211),
        (Bits16, &[0x67, 0x67][..], 0xaaaa_6655),
    ] {
        // The same r/m bits select [BX+SI] in 16-bit addressing and [EAX] in 32-bit.
        let code = [prefixes, &[0x8b, 0x08]].concat();
        let mut image = Image::new(&code);
        image.cpu = CpuState::protected_mode(default, Bits32);
        image.cpu.eip = 0x1000;
        image.cpu.registers.eax = 0x5000;
        image.cpu.registers.ebx = 0xdead_2000;
        image.cpu.registers.esi = 0xbeef_0010;
        image.cpu.registers.ecx = 0xaaaa_aaaa;
        image.map(2, 0x4000, false);
        image.map(5, 0x5000, false);
        image.data(0x4010, &[0x11, 0x22, 0x33, 0x44]);
        image.data(0x5000, &[0x55, 0x66, 0x77, 0x88]);
        let mut expected = image.cpu;
        expected.registers.ecx = expected_ecx;
        expected.eip += code.len() as u32;
        expected.instruction_count += 1;
        assert_eq!(step(&image), expected);
    }
}

#[test]
fn live_address_size_controls_sib_consumption_and_loop_counter() {
    use SegmentDefaultSize::{Bits16, Bits32};
    for (default, result, length) in [(Bits16, 0xaaaa_2211, 2), (Bits32, 0x8877_6655, 3)] {
        let mut image = Image::new(&[0x8b, 0x04, 0x8b]);
        image.cpu = CpuState::protected_mode(default, Bits32);
        image.cpu.eip = 0x1000;
        image.cpu.registers.eax = 0xaaaa_aaaa;
        image.cpu.registers.esi = 0x2010;
        image.cpu.registers.ebx = 0x5000;
        image.cpu.registers.ecx = 3;
        image.map(2, 0x4000, false);
        image.map(5, 0x5000, false);
        image.data(0x4010, &[0x11, 0x22]);
        image.data(0x500c, &[0x55, 0x66, 0x77, 0x88]);
        let mut expected = image.cpu;
        expected.registers.eax = result;
        expected.eip += length;
        expected.instruction_count += 1;
        assert_eq!(step(&image), expected);

        let mut image = Image::new(&[0xe2, 0]);
        image.cpu = CpuState::protected_mode(default, Bits32);
        image.cpu.eip = 0x1000;
        image.cpu.registers.ecx = 0xabcd_0000;
        let mut expected = image.cpu;
        expected.registers.ecx = if default == Bits16 {
            0xabcd_ffff
        } else {
            0xabcc_ffff
        };
        expected.eip += 2;
        expected.instruction_count += 1;
        assert_eq!(step(&image), expected);
    }
}

#[test]
fn absolute_offsets_follow_address_width_and_stack_offsets_follow_ss() {
    use SegmentDefaultSize::{Bits16, Bits32};
    for (default, length) in [(Bits16, 3), (Bits32, 5)] {
        // MOV AL, moffs uses a fixed data width and a variable address field.
        let mut image = Image::new(&[0xa0, 0x10, 0x20, 0x01, 0]);
        image.cpu = CpuState::protected_mode(default, Bits32);
        image.cpu.eip = 0x1000;
        image.map(2, 0x4000, false);
        image.map(0x12, 0x5000, false);
        image.data(0x4010, &[0x11]);
        image.data(0x5010, &[0x22]);
        let mut expected = image.cpu;
        expected.registers.eax = if default == Bits16 { 0x11 } else { 0x22 };
        expected.eip += length;
        expected.instruction_count += 1;
        assert_eq!(step(&image), expected);
    }
    for (code, stack, esp, page, result, next_esp) in [
        (Bits16, Bits32, 0x0001_2010, 0x12, 0xaaaa_2211, 0x0001_2012),
        (Bits32, Bits16, 0xdead_2010, 2, 0x4433_2211, 0xdead_2014),
    ] {
        // 67 does not change the implicit stack pointer used by POP.
        let mut image = Image::new(&[0x67, 0x58]);
        image.cpu = CpuState::protected_mode(code, stack);
        image.cpu.eip = 0x1000;
        image.cpu.registers.eax = 0xaaaa_aaaa;
        image.cpu.registers.esp = esp;
        image.map(page, 0x4000, false);
        image.data(0x4010, &[0x11, 0x22, 0x33, 0x44]);
        let mut expected = image.cpu;
        expected.registers.eax = result;
        expected.registers.esp = next_esp;
        expected.eip += 2;
        expected.instruction_count += 1;
        assert_eq!(step(&image), expected);
    }
}

#[test]
fn mandatory_operand_prefix_keeps_its_opcode_role_for_both_defaults() {
    for default in [SegmentDefaultSize::Bits16, SegmentDefaultSize::Bits32] {
        // 66 selects MOVD to XMM; it does not turn its dword input into a word.
        let mut image = Image::new(&[0x66, 0x0f, 0x6e, 0xc0]);
        image.cpu = CpuState::protected_mode(default, SegmentDefaultSize::Bits32);
        image.cpu.eip = 0x1000;
        image.cpu.registers.eax = 0x1234_5678;
        image.cpu.simd.xmm[0] = [0xaa; 16];
        let mut expected = image.cpu;
        expected.simd.xmm[0] = [0; 16];
        expected.simd.xmm[0][..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        expected.eip += 4;
        expected.instruction_count += 1;
        assert_eq!(step(&image), expected);
    }
}
