use super::*;

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
fn context_changes_and_known_faults_do_not_discover_fallthrough() {
    for (profile, code) in [
        (ExecutionProfile::Real16, &[0xea, 0, 0, 0, 0][..]), // far JMP
        (ExecutionProfile::Real16, &[0x9a, 0, 0, 0, 0]),     // far CALL
        (ExecutionProfile::Real16, &[0xcb]),                 // far RET
        (ExecutionProfile::Real16, &[0xcf]),                 // IRET
        (ExecutionProfile::Real16, &[0xcd, 0x10]),           // INT
        (ExecutionProfile::Real16, &[0x0f, 0x0b]),           // UD2
        (SegmentProfile::Flat32.into(), &[0xec]),            // I/O is privileged in this profile
        (SegmentProfile::Flat32.into(), &[0xfb]),            // STI is privileged too
        (SegmentProfile::Flat32.into(), &[0xf4]),            // HLT requires CPL0
    ] {
        let mut bytes = code.to_vec();
        bytes.push(0x0f); // An incomplete suffix detects accidental exploration.
        let blocks = discover::blocks(
            0x1000,
            Compiler::new(profile).target(),
            FunctionLimits::default(),
            reader(0x1000, &bytes),
        )
        .unwrap();
        assert_eq!(blocks.len(), 1, "{profile:?}, {code:02x?}");
        assert_eq!(blocks[0].entry.byte_len, code.len());
    }
}

#[test]
fn discovered_continuations_preserve_existing_dispatch_and_runtime_exits() {
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
    assert_eq!(actual.bytes, expected.bytes);
}
