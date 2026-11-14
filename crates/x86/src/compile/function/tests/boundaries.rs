use super::*;

#[test]
fn decode_failures_preserve_valid_prefixes_and_other_branches() {
    let limits = FunctionLimits::default();
    for suffix in [
        &[0xb8][..],         // Incomplete MOV EAX,imm32
        &[0xf1, 0x90, 0xc3], // Unsupported opcode followed by valid instructions
        &[0x66; 16],         // Overlong instruction
    ] {
        let mut prefix = vec![0x90];
        prefix.extend(suffix);
        assert_eq!(entries(&prefix, limits), [(0x1000, 1)]);

        let mut branch = vec![
            0x75, 3,    // JNZ 1005, explored before the fallthrough
            0x40, // INC EAX
            0xff, 0xe0, // JMP EAX
        ];
        branch.extend(suffix);
        assert_eq!(entries(&branch, limits), [(0x1000, 2), (0x1002, 3)]);
    }
}

#[test]
fn undecodable_entries_still_report_the_decode_error() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    for (code, expected) in [
        (
            &[0xb8][..],
            BlockError::TruncatedInstruction {
                address: 0x1000,
                available: 1,
            },
        ),
        (
            &[0xf1],
            BlockError::UnsupportedInstruction {
                address: 0x1000,
                opcode: 0xf1,
            },
        ),
        (
            &[0x66; 16],
            BlockError::InstructionTooLong { address: 0x1000 },
        ),
    ] {
        assert_eq!(
            compiler
                .compile_function(0x1000, FunctionLimits::default(), reader(0x1000, code))
                .err(),
            Some(expected),
        );
    }
}

#[test]
fn block_boundaries_with_known_fallthrough_continue_discovery() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    for code in [
        &[0xec][..],                  // IN AL,DX
        &[0xee],                      // OUT DX,AL
        &[0x6c],                      // INSB
        &[0xf3, 0x6c],                // REP INSB
        &[0x6e],                      // OUTSB
        &[0xf3, 0x6e],                // REP OUTSB
        &[0x8e, 0xd8],                // MOV DS,AX
        &[0x8e, 0xd0],                // MOV SS,AX
        &[0x1f],                      // POP DS
        &[0x17],                      // POP SS
        &[0xc5, 0x1e, 0, 0x20],       // LDS BX,[2000]
        &[0x0f, 0xb2, 0x1e, 0, 0x20], // LSS BX,[2000]
        &[0x0f, 0xa2],                // CPUID
        &[0xfb],                      // STI
        &[0x9d],                      // POPF
        &[0xf4],                      // HLT
    ] {
        let mut bytes = code.to_vec();
        bytes.extend([0x90, 0xc3]); // NOP; RET after the execution boundary
        let blocks = discover::blocks(
            0x1000,
            target,
            FunctionLimits::default(),
            reader(0x1000, &bytes),
            |_, _| false,
        )
        .unwrap();
        assert_eq!(
            blocks
                .iter()
                .map(|block| (block.entry.start_eip, block.entry.byte_len))
                .collect::<Vec<_>>(),
            [(0x1000, code.len()), (0x1000 + code.len() as u32, 2)],
            "{code:02x?}",
        );
    }
}

#[test]
fn real_mode_far_calls_discover_only_the_return_continuation() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    for call in [
        &[0x9a, 0, 0x20, 0, 0x30][..],         // CALL FAR 3000:2000
        &[0xff, 0x1e, 0, 0x20],                // CALL FAR [2000]
        &[0x66, 0x9a, 0, 0x20, 0, 0, 0, 0x30], // CALL FAR with a dword return offset
        &[0x66, 0xff, 0x1e, 0, 0x20],          // Indirect dword far call
    ] {
        let mut bytes = call.to_vec();
        bytes.extend([0x40, 0xcb, 0x90]); // INC AX; RETF; unused NOP
        let mut read = reader(0x1000, &bytes);
        let blocks = discover::blocks(
            0x1000,
            target,
            FunctionLimits::default(),
            |eip, output| {
                assert!((0x1000..=0x1000 + bytes.len() as u32).contains(&eip));
                read(eip, output)
            },
            |_, _| false,
        )
        .unwrap();
        assert_eq!(
            blocks
                .iter()
                .map(|block| (block.entry.start_eip, block.entry.byte_len))
                .collect::<Vec<_>>(),
            [(0x1000, call.len()), (0x1000 + call.len() as u32, 2)],
            "{call:02x?}",
        );
    }
}

#[test]
fn real_mode_far_call_continuations_use_the_saved_offset_width() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    for (start, call, continuation) in [
        (0xfffb, &[0x9a, 0, 0x20, 0, 0x30][..], 0),
        (0xfffc, &[0xff, 0x1e, 0, 0x20], 0),
        (0xfff8, &[0x66, 0x9a, 0, 0x20, 0, 0, 0, 0x30], 0x10000),
        (0xfffb, &[0x66, 0xff, 0x1e, 0, 0x20], 0x10000),
    ] {
        let (instruction, _) = snapshot(call, start, SegmentDefaultSize::Bits16).unwrap();
        assert_eq!(instruction.successors(target), [Some(continuation), None]);
    }
}

#[test]
fn context_changes_and_known_faults_do_not_discover_fallthrough() {
    for (profile, code) in [
        (ExecutionProfile::Real16, &[0xea, 0, 0, 0, 0][..]), // far JMP
        (SegmentProfile::Segmented16.into(), &[0x9a, 0, 0, 0, 0]), // far CALL
        (SegmentProfile::Segmented16.into(), &[0xff, 0x1e, 0, 0]), // indirect far CALL
        (SegmentProfile::Flat32.into(), &[0x9a, 0, 0, 0, 0, 0, 0]), // far CALL
        (SegmentProfile::Flat32.into(), &[0xff, 0x1d, 0, 0, 0, 0]), // indirect far CALL
        (ExecutionProfile::Real16, &[0xcb]),                 // far RET
        (ExecutionProfile::Real16, &[0xcf]),                 // IRET
        (ExecutionProfile::Real16, &[0xcd, 0x10]),           // INT
        (ExecutionProfile::Real16, &[0x0f, 0x0b]),           // UD2
        (SegmentProfile::Flat32.into(), &[0xec]),            // I/O is privileged in this profile
        (SegmentProfile::Flat32.into(), &[0xfb]),            // STI is privileged too
        (SegmentProfile::Flat32.into(), &[0xf4]),            // HLT requires CPL0
    ] {
        let mut bytes = code.to_vec();
        bytes.extend([0x90, 0xc3]); // A valid suffix exposes accidental exploration.
        let blocks = discover::blocks(
            0x1000,
            Compiler::new(profile).target(),
            FunctionLimits::default(),
            reader(0x1000, &bytes),
            |_, _| false,
        )
        .unwrap();
        assert_eq!(blocks.len(), 1, "{profile:?}, {code:02x?}");
        assert_eq!(blocks[0].entry.byte_len, code.len());
    }
}

#[test]
fn discovered_continuations_preserve_snapshot_entry_spans() {
    let code = [0xec, 0x8e, 0xd8, 0x8e, 0xd0, 0x0f, 0xa2, 0xc3];
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let actual = compiler
        .compile_function(0x1000, FunctionLimits::default(), reader(0x1000, &code))
        .unwrap();
    let snapshots: Vec<_> = [(0, 1), (1, 2), (3, 2), (5, 2), (7, 1)]
        .into_iter()
        .map(|(offset, len)| BlockSnapshot {
            start_eip: 0x1000 + offset as u32,
            bytes: &code[offset..offset + len],
            instruction_limit: 1,
        })
        .collect();
    let expected = compiler.compile_blocks(&snapshots).unwrap();
    assert_eq!(actual.entries, expected.entries);
}

#[test]
fn excluded_encodings_keep_discovered_successors() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let code = [0xb8, 1, 0, 0xeb, 2, 0x90, 0x90, 0x40, 0xc3];
    let mut module = compiler.function_module();
    let entries = module
        .add_function_excluding(
            CodeAddress::CsRelative(0x1000),
            FunctionLimits::default(),
            reader(0x1000, &code),
            |eip, bytes| eip <= 0x1001 && 0x1001 < eip + bytes as u32,
        )
        .unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|block| (block.start_eip, block.byte_len))
            .collect::<Vec<_>>(),
        [(0x1003, 2), (0x1007, 2)]
    );
    module.compile().unwrap();
}
