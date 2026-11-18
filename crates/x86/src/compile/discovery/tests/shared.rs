use super::*;
use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::TestModule,
    },
    CpuState, Segment, SegmentRegister,
};

fn address(eip: u32) -> CodeAddress {
    CodeAddress::RealMode { segment: 0x10, eip }
}

#[test]
fn two_roots_jump_to_one_shared_tail() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let size = SegmentDefaultSize::Bits16;
    let first = (address(0x100), &[0xeb, 0x1e][..]); // JMP 0120
    let second = (address(0x110), &[0xeb, 0x0e][..]); // JMP 0120
    let tail = (address(0x120), &[0xba, 7, 0, 0xff, 0xe3][..]); // MOV DX,7; JMP BX
    let mut read = mapped_code([first, second, tail]);
    let mut tail_reads = 0;
    let mut module = compiler.discover_blocks(|address, output| {
        if address == tail.0 {
            tail_reads += 1;
        }
        read(address, output)
    });
    module
        .add_root(first.0, size, DiscoveryLimits::default())
        .unwrap();
    // Only the second root's JMP is new.
    let short = DiscoveryLimits {
        instructions: 1,
        ..DiscoveryLimits::default()
    };
    module.add_root(second.0, size, short).unwrap();
    let blocks = module.compile().unwrap();
    assert_eq!(tail_reads, 1);
    assert_eq!(
        blocks
            .entries
            .iter()
            .map(|entry| entry.export.as_str())
            .collect::<Vec<_>>(),
        ["block_0010:0100", "block_0010:0110"]
    );

    assert_eq!(blocks.spans, [(first.0, 2), (second.0, 2), (tail.0, 5)]);
    let wasm = relocade_test_support::Module::new(&blocks.bytes);
    assert!(wasm.wasmtime().get_export("block_0010:0120").is_none());
    for (root, _) in [first, second] {
        let entry = TestModule::from_bytes(
            &blocks.bytes,
            &format!("block_{root}"),
            Some(blocks.execution_target),
        );
        let mut image = Image::empty();
        image.cpu = CpuState::real_mode();
        image.cpu.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x10);
        image.cpu.eip = root.eip();
        image.cpu.registers.ebx = 0x200;
        image.map(0, 0, false);
        for (address, bytes) in [first, second, tail] {
            image.data(0x100 + address.eip(), bytes);
        }
        let mut after = image.cpu;
        after.eip = 0x200;
        after.instruction_count = 3;
        after.registers.edx = 7;
        assert_eq!(
            entry.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[],
                    exit: Exit::Dispatch(0x200)
                }]
            ),
        );
    }
}

#[test]
fn overlapping_roots_split_at_the_first_common_instruction_in_either_order() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    // At 0100 the NOP bytes are MOV AX's immediate. At 0101 they execute.
    // Both streams fall through to 0103 without branching there.
    let bytes = [0xb8, 0x90, 0x90, 0x42, 0xc3];
    for roots in [[0x100, 0x101, 0x104], [0x104, 0x101, 0x100]] {
        let mut code = walk::Discovery::new(
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
                .map(|block| (block.address.eip(), block.byte_len))
                .collect::<Vec<_>>(),
            [(0x100, 3), (0x101, 2), (0x103, 1), (0x104, 1)]
        );
    }
}

#[test]
fn a_new_root_spends_its_budget_only_on_the_jump_to_an_accepted_suffix() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let mut code = walk::Discovery::new(
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
            .map(|block| (block.address.eip(), block.byte_len))
            .collect::<Vec<_>>(),
        [(0x100, 1), (0x101, 2), (0x103, 2), (0x110, 2)]
    );
}

#[test]
fn a_rejected_branch_cannot_split_an_accepted_block() {
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let mut code = walk::Discovery::new(
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
    assert_eq!((blocks[0].address.eip(), blocks[0].byte_len), (0x100, 3));
}

#[test]
fn a_discovered_callee_keeps_its_call_slot_with_one_or_several_roots() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let ranges: [(CodeAddress, &[u8]); 4] = [
        (address(0x100), &[0x74, 0x4e, 0xeb, 0x1c]), // JZ 0150; JMP 0120
        (address(0x110), &[0xeb, 0x0e]),             // JMP 0120
        (address(0x120), &[0xe8, 0x2d, 0, 0xc3]),    // CALL 0150; RET
        (address(0x150), &[0xc3]),
    ];
    let roots = [address(0x100), address(0x110)];
    for requested in [&roots[..1], &roots[..]] {
        let mut module = compiler.discover_blocks(mapped_code(ranges));
        for &root in requested {
            module
                .add_root(root, SegmentDefaultSize::Bits16, DiscoveryLimits::default())
                .unwrap();
        }
        let blocks = module.compile().unwrap();
        assert_eq!(blocks.call_targets, [0x0010_0150]);
        let exported: Vec<_> = blocks.entries.iter().map(|entry| entry.address).collect();
        let mut expected = requested.to_vec();
        expected.push(address(0x123)); // RET resumes the caller here.
        assert_eq!(exported, expected);
    }
}
