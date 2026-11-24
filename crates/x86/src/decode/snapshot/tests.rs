use crate::support::config::{FLAT32, REAL16, SEGMENTED16};
use crate::{
    BlockError, CallTarget, Compiler, CpuFeature, CpuModel, InstructionFlow,
    SegmentDefaultSize::{Bits16, Bits32},
    SnapshotInstruction,
};

#[test]
fn decoding_keeps_the_captured_width_and_wrapping_encoding_end() {
    let eip = u32::MAX - 2;
    let bytes = [0xb8, 0x34, 0x12, 0x78, 0x56, 0xc3]; // MOV AX/EAX,imm, with trailing bytes
    for (size, next) in [(Bits16, 0), (Bits32, 2)] {
        let instruction = SnapshotInstruction::decode(&bytes, eip, size).unwrap();
        assert_eq!(instruction.eip(), eip);
        assert_eq!(instruction.code_default_size(), size);
        assert_eq!(instruction.next_eip(), next);
        assert_eq!(instruction.flow(FLAT32), InstructionFlow::Continue);
    }
    assert_eq!(
        SnapshotInstruction::decode(&bytes[..3], eip, Bits32).err(),
        Some(BlockError::TruncatedInstruction {
            address: eip,
            available: 3
        })
    );
}

#[test]
fn near_transfers_report_operand_sized_targets_and_return_offsets() {
    use InstructionFlow::{Branch, Jump, NearCall, Stop};

    let config = FLAT32;
    for code in [
        &[0x75, 0xfe][..],                     // JNZ rel8
        &[0xe3, 0xfe],                         // JECXZ
        &[0xe2, 0xfe],                         // LOOP
        &[0xe1, 0xfe],                         // LOOPE
        &[0xe0, 0xfe],                         // LOOPNE
        &[0x0f, 0x85, 0xfa, 0xff, 0xff, 0xff], // JNZ rel32
    ] {
        let instruction = SnapshotInstruction::decode(code, 0x1000, Bits32).unwrap();
        assert_eq!(instruction.flow(config), Branch { taken: 0x1000 });
        assert_eq!(instruction.next_eip(), 0x1000 + code.len() as u32);
    }
    for (code, expected) in [
        (&[0x66, 0xeb, 0xff][..], Jump { target: 1 }),
        (&[0x66, 0xe9, 0xfe, 0xff], Jump { target: 1 }),
        (&[0x66, 0x75, 0xff], Branch { taken: 1 }),
        (&[0x66, 0xe8, 0x10, 0], NearCall { return_eip: 3 }),
        (&[0x66, 0xff, 0xd0], NearCall { return_eip: 2 }),
        (&[0xff, 0xe0], Stop), // JMP EAX
        (&[0xc3], Stop),       // RET
    ] {
        let instruction = SnapshotInstruction::decode(code, 0xffff, Bits32).unwrap();
        assert_eq!(instruction.flow(config), expected, "{code:02x?}");
        assert_eq!(instruction.next_eip(), 0xffff + code.len() as u32);
    }
    let instruction = SnapshotInstruction::decode(&[0x66, 0xeb, 0], 0xffff, Bits16).unwrap();
    assert_eq!(instruction.flow(REAL16), Jump { target: 0x10002 });
}

#[test]
fn far_calls_report_the_operand_sized_return_offset_in_both_modes() {
    for (start, code, return_eip) in [
        (0xfffb, &[0x9a, 0, 0x20, 0, 0x30][..], 0),
        (0xfffc, &[0xff, 0x1e, 0, 0x20], 0),
        (0xfff8, &[0x66, 0x9a, 0, 0x20, 0, 0, 0, 0x30], 0x10000),
        (0xfffb, &[0x66, 0xff, 0x1e, 0, 0x20], 0x10000),
    ] {
        let instruction = SnapshotInstruction::decode(code, start, Bits16).unwrap();
        assert_eq!(instruction.next_eip(), 0x10000);
        for config in [REAL16, SEGMENTED16] {
            assert_eq!(
                instruction.flow(config),
                InstructionFlow::FarCall { return_eip },
                "{config:?}, {code:02x?}"
            );
        }
    }
}

#[test]
fn context_changes_and_traps_end_the_block() {
    let config = REAL16;
    for code in [
        &[0xea, 0, 0x20, 0, 0x30][..], // JMP FAR 3000:2000
        &[0xff, 0x2e, 0, 0x20],        // JMP FAR [2000]
        &[0xcb],                       // RETF
        &[0xcf],                       // IRET
        &[0xcd, 0x10],                 // INT 10
        &[0x0f, 0x0b],                 // UD2
    ] {
        let instruction = SnapshotInstruction::decode(code, 0, Bits16).unwrap();
        assert_eq!(
            instruction.flow(config),
            InstructionFlow::Stop,
            "{code:02x?}"
        );
    }
}

#[test]
fn a_fault_guaranteed_by_the_compiler_config_ends_discovery() {
    let flat = Compiler::new(FLAT32);
    let real = Compiler::new(REAL16);
    let halt = SnapshotInstruction::decode(&[0xf4], 0, Bits16).unwrap();
    assert_eq!(halt.flow(real.config()), InstructionFlow::Fallthrough);
    assert_eq!(halt.flow(flat.config()), InstructionFlow::Stop);

    let rdtsc = SnapshotInstruction::decode(&[0x0f, 0x31], 0, Bits32).unwrap();
    assert_eq!(rdtsc.flow(flat.config()), InstructionFlow::Continue);
    let without_tsc = CpuModel::default().with_feature(CpuFeature::Tsc, false);
    assert_eq!(
        rdtsc.flow(flat.with_cpu_model(without_tsc).config()),
        InstructionFlow::Stop
    );
}

#[test]
fn cpl3_cli_ends_discovery_only_when_iopl_is_guaranteed() {
    let cli = SnapshotInstruction::decode(&[0xfa], 0, Bits16).unwrap();
    let hosted = crate::support::config::SEGMENTED16;
    assert_eq!(cli.flow(hosted), InstructionFlow::Stop);
    let pc = crate::support::config::physical_with_live_flags(hosted.entry);
    assert_eq!(cli.flow(pc), InstructionFlow::Continue);
}

#[test]
fn direct_calls_report_their_encoded_destination() {
    for (code, expected_target) in [
        (&[0x66, 0xe8, 0xfd, 0xff][..], Some(CallTarget::Near(0))),
        (
            &[0x9a, 0x78, 0x56, 0x34, 0x12, 0xbc, 0x9a],
            Some(CallTarget::Far {
                selector: 0x9abc,
                offset: 0x12345678,
            }),
        ),
        (&[0xff, 0xd0], None), // CALL EAX
        (&[0xff, 0x18], None), // CALL FAR [EAX]
        (&[0xeb, 0xfe], None), // JMP rel8
    ] {
        let instruction = SnapshotInstruction::decode(code, 0xffff, Bits32).unwrap();
        assert_eq!(instruction.call_target(), expected_target, "{code:02x?}");
    }
}
