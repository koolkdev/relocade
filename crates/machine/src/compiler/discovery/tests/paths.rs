use super::*;

#[test]
fn decode_errors_reject_roots_and_preserve_valid_prefixes_and_sibling_paths() {
    let entry = EntryAssumptions::FLAT32;
    let limits = DiscoveryLimits::default();
    for (suffix, expected) in [
        (
            &[0xb8][..],
            BlockError::TruncatedInstruction {
                address: START,
                available: 1,
            },
        ),
        (
            &[0xf1, 0x90, 0xc3],
            BlockError::UnsupportedInstruction {
                address: START,
                opcode: 0xf1,
            },
        ),
        (
            &[0x66; 16],
            BlockError::InstructionTooLong { address: START },
        ),
    ] {
        assert!(
            matches!(discover(entry, Bits32, suffix, limits), Err(Error::Compile(error)) if error == expected)
        );

        let mut prefix = vec![0x90];
        prefix.extend(suffix);
        assert_eq!(
            spans(&discover(entry, Bits32, &prefix, limits).unwrap()),
            [(START, 1)]
        );

        let mut branch = vec![
            0x75, 3,    // JNZ 1005
            0x40, // INC EAX
            0xff, 0xe0, // JMP EAX
        ];
        branch.extend(suffix);
        assert_eq!(
            spans(&discover(entry, Bits32, &branch, limits).unwrap()),
            [(0x1000, 2), (0x1002, 3)]
        );
    }
}

#[test]
fn unreadable_targets_and_overlapping_encodings_end_only_the_affected_path() {
    let entry = EntryAssumptions::FLAT32;
    for (code, expected) in [
        (&[0x75, 0x7f, 0xc3][..], &[(0x1000, 2), (0x1002, 1)][..]),
        // Backward jump into the preceding MOV's immediate.
        (&[0xb8, 0x90, 0x90, 0x90, 0x90, 0xeb, 0xfb], &[(0x1000, 7)]),
        // The taken path starts in the fallthrough MOV's immediate.
        (
            &[0x75, 1, 0xb8, 0x90, 0x90, 0x90, 0xc3],
            &[(0x1000, 2), (0x1003, 4)],
        ),
    ] {
        let blocks = discover(entry, Bits32, code, DiscoveryLimits::default()).unwrap();
        assert_eq!(spans(&blocks), expected, "{code:02x?}");
    }
}

#[test]
fn shared_instruction_offsets_are_decoded_in_each_requested_width() {
    let config = CompilerConfig::new(EntryAssumptions::SEGMENTED32, MemoryBackend::HostPageTable);
    let root = address(START);
    let code = [0xb8, 0x34, 0x12, 0xc3, 0x90, 0xc3];
    let mut discovery = BlockDiscovery::new(config, mapped_code([(root, code.as_slice())]));
    for size in [Bits16, Bits32] {
        discovery
            .add_root(root, size, DiscoveryLimits::default())
            .unwrap();
    }
    let blocks = discovery.finish().unwrap();
    let sizes: Vec<_> = blocks
        .iter()
        .map(|block| (block.address(), block.code_default_size(), block.byte_len()))
        .collect();
    assert_eq!(sizes, [(root, Bits16, 4), (root, Bits32, 6)]);
}

#[test]
fn io_and_segment_loads_split_fallthrough_until_a_known_fault() {
    let limits = DiscoveryLimits::default();
    let code = [
        0xec, // IN AL,DX
        0x8e, 0xd8, // MOV DS,AX
        0xf3, 0x6e, // REP OUTSB
        0x17, // POP SS
        0x0f, 0xa2, // CPUID
        0x0f, 0x0b, // UD2
        0x90, 0xc3, // NOP; RET beyond the fault
    ];
    let real = discover(EntryAssumptions::REAL_MODE, Bits16, &code, limits).unwrap();
    assert_eq!(
        spans(&real),
        [
            (0x1000, 1),
            (0x1001, 2),
            (0x1003, 2),
            (0x1005, 1),
            (0x1006, 2),
            (0x1008, 2)
        ]
    );
    assert!(real[0].is_exported());
    assert!(real[1..].iter().all(|block| !block.is_exported()));

    // The Flat32 entry faults at the first IN, before discovering any successor.
    let flat = discover(EntryAssumptions::FLAT32, Bits32, &code, limits).unwrap();
    assert_eq!(spans(&flat), [(0x1000, 1)]);
}
