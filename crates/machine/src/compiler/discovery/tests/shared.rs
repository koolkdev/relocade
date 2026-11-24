use super::*;

#[test]
fn two_roots_share_one_tail_without_rereading_or_charging_for_it() {
    let first = address(0x100);
    let second = address(0x110);
    let tail = address(0x120);
    let mut read = mapped_code([
        (first, &[0xeb, 0x1e][..]),        // JMP 0120
        (second, &[0xeb, 0x0e]),           // JMP 0120
        (tail, &[0xba, 7, 0, 0xff, 0xe3]), // MOV DX,7; JMP BX
    ]);
    let mut tail_reads = 0;
    let config = CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus);
    let mut discovery = BlockDiscovery::new(config, |address, bytes: &mut [u8]| {
        if address == tail {
            tail_reads += 1;
        }
        read(address, bytes)
    });
    discovery
        .add_root(first, Bits16, DiscoveryLimits::default())
        .unwrap();
    discovery
        .add_root(
            second,
            Bits16,
            DiscoveryLimits {
                instructions: 1,
                bytes: 2,
                ..Default::default()
            },
        )
        .unwrap();
    let blocks = discovery.finish().unwrap();
    assert_eq!(tail_reads, 1);
    assert_eq!(spans(&blocks), [(0x100, 2), (0x110, 2), (0x120, 5)]);
}

#[test]
fn overlapping_roots_split_at_their_first_common_instruction_in_either_order() {
    // The MOV immediate at 0101 is also a stream of NOPs. Both paths reach INC at 0103.
    let code = [0xb8, 0x90, 0x90, 0x42, 0xc3];
    let config = CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus);
    for roots in [[0x100, 0x101, 0x104], [0x104, 0x101, 0x100]] {
        let mut discovery =
            BlockDiscovery::new(config, mapped_code([(address(0x100), code.as_slice())]));
        for eip in roots {
            discovery
                .add_root(address(eip), Bits16, DiscoveryLimits::default())
                .unwrap();
        }
        let blocks = discovery.finish().unwrap();
        assert_eq!(
            spans(&blocks),
            [(0x100, 3), (0x101, 2), (0x103, 1), (0x104, 1)]
        );
    }
}

#[test]
fn a_new_root_can_split_an_accepted_suffix_without_reading_it_again() {
    let suffix = address(0x100);
    let jump = address(0x110);
    let code = [0x90, 0x90, 0x90, 0x90, 0xc3];
    let mut read = mapped_code([(suffix, code.as_slice()), (jump, &[0xeb, 0xef])]);
    let config = CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus);
    let mut reads = Vec::new();
    let mut discovery = BlockDiscovery::new(config, |address, bytes: &mut [u8]| {
        reads.push(address);
        read(address, bytes)
    });
    discovery
        .add_root(suffix, Bits16, DiscoveryLimits::default())
        .unwrap();
    let limits = DiscoveryLimits {
        instructions: 1,
        bytes: 2,
        block_instructions: 1,
    };
    discovery.add_root(jump, Bits16, limits).unwrap();
    discovery
        .add_root(
            address(0x103),
            Bits16,
            DiscoveryLimits { bytes: 1, ..limits },
        )
        .unwrap();
    let blocks = discovery.finish().unwrap();
    let jump_read = reads.iter().position(|&address| address == jump).unwrap();
    assert!(reads[jump_read..]
        .iter()
        .all(|address| address.eip() >= 0x110));
    assert_eq!(
        spans(&blocks),
        [(0x100, 1), (0x101, 2), (0x103, 2), (0x110, 2)]
    );
}

#[test]
fn an_over_budget_branch_leaves_the_accepted_block_unsplit() {
    let root = address(0x100);
    let branch = address(0x110);
    let mut discovery = BlockDiscovery::new(
        CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus),
        mapped_code([
            (root, &[0x90, 0x90, 0xc3][..]),
            (branch, &[0x75, 0xef, 0x90, 0xc3]), // JNZ 0101; NOP; RET
        ]),
    );
    discovery
        .add_root(root, Bits16, DiscoveryLimits::default())
        .unwrap();
    assert!(matches!(
        discovery.add_root(
            branch,
            Bits16,
            DiscoveryLimits {
                instructions: 2,
                bytes: 3,
                ..Default::default()
            }
        ),
        Err(Error::Discovery(DiscoveryError::TooLarge {
            address: 0x110
        }))
    ));
    let blocks = discovery.finish().unwrap();
    assert_eq!(spans(&blocks), [(0x100, 3)]);
}
