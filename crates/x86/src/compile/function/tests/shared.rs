use super::*;

#[test]
fn overlapping_roots_split_at_the_first_common_instruction_in_either_order() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    // At 0100 the NOP bytes are MOV AX's immediate. At 0101 they execute.
    // Both streams fall through to 0103 without branching there.
    let bytes = [0xb8, 0x90, 0x90, 0x42, 0xc3];
    for roots in [[0x100, 0x101, 0x104], [0x104, 0x101, 0x100]] {
        let mut code = discover::Discovery::new(
            CodeAddress::CsRelative(roots[0]),
            SegmentDefaultSize::Bits16,
            target,
        );
        for eip in roots {
            code.add_root(eip, DiscoveryLimits::default(), reader(0x100, &bytes))
                .unwrap();
        }
        assert_eq!(
            code.finish()
                .iter()
                .map(|block| (block.entry.address.eip(), block.entry.byte_len))
                .collect::<Vec<_>>(),
            [(0x100, 3), (0x101, 2), (0x103, 1), (0x104, 1)]
        );
    }
}

#[test]
fn a_new_root_spends_its_budget_only_on_the_jump_to_an_accepted_suffix() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let mut code = discover::Discovery::new(
        CodeAddress::CsRelative(0x100),
        SegmentDefaultSize::Bits16,
        target,
    );
    let suffix = [0x90, 0x90, 0x90, 0x90, 0xc3];
    code.add_root(0x100, DiscoveryLimits::default(), reader(0x100, &suffix))
        .unwrap();

    let limits = DiscoveryLimits {
        instructions: 1,
        bytes: 2,
        block_instructions: 1,
    };
    let mut read_jump = reader(0x110, &[0xeb, 0xef]); // JMP 0101
    code.add_root(0x110, limits, |eip, output| {
        assert!(eip >= 0x110, "reread the accepted suffix at {eip:04x}");
        read_jump(eip, output)
    })
    .unwrap();
    code.add_root(0x103, DiscoveryLimits { bytes: 1, ..limits }, |_, _| {
        panic!("an entry at an accepted instruction needs no code read")
    })
    .unwrap();
    assert_eq!(
        code.finish()
            .iter()
            .map(|block| (block.entry.address.eip(), block.entry.byte_len))
            .collect::<Vec<_>>(),
        [(0x100, 1), (0x101, 2), (0x103, 2), (0x110, 2)]
    );
}

#[test]
fn a_rejected_branch_cannot_split_an_accepted_block() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let mut code = discover::Discovery::new(
        CodeAddress::CsRelative(0x100),
        SegmentDefaultSize::Bits16,
        target,
    );
    code.add_root(
        0x100,
        DiscoveryLimits::default(),
        reader(0x100, &[0x90, 0x90, 0xc3]),
    )
    .unwrap();
    // JNZ 0101 joins the accepted block. Its fallthrough fits the NOP but not the RET.
    let branch = [0x75, 0xef, 0x90, 0xc3];
    let limits = DiscoveryLimits {
        instructions: 2,
        bytes: 3,
        ..DiscoveryLimits::default()
    };
    assert_eq!(
        code.add_root(0x110, limits, reader(0x110, &branch)),
        Err(BlockError::DiscoveryTooLarge { address: 0x110 })
    );
    let blocks = code.finish();
    assert_eq!(blocks.len(), 1);
    assert_eq!(
        (blocks[0].entry.address.eip(), blocks[0].entry.byte_len),
        (0x100, 3)
    );
}
