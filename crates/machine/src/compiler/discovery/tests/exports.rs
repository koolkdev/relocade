use super::*;

fn exports(blocks: &[DecodedBlock]) -> Vec<u32> {
    blocks
        .iter()
        .filter(|block| block.is_exported())
        .map(|block| block.address().eip())
        .collect()
}

#[test]
fn straight_line_splits_continue_discovery_and_stay_private() {
    let code = [0xb8, 1, 0, 0, 0, 0x90, 0x90, 0x90, 0xc3];
    let blocks = discover(
        SegmentProfile::Flat32.into(),
        Bits32,
        &code,
        DiscoveryLimits {
            block_instructions: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(spans(&blocks), [(0x1000, 6), (0x1006, 2), (0x1008, 1)]);
    assert_eq!(exports(&blocks), [START]);
}

#[test]
fn branches_stay_private_and_near_calls_export_their_continuations() {
    let root = address(START);
    let code = [
        0x90, 0x90, 0x75, 0xfd, // NOP; loop at the second NOP
        0xe8, 0xf7, 0x0f, 0, 0, // CALL 2000
        0xff, 0xd0, // CALL EAX
        0xc3, 0x90, // RET; unused NOP
    ];
    let mut read = mapped_code([(root, code.as_slice()), (address(0x2000), &[0xc3])]);
    let target = Compiler::new(SegmentProfile::Flat32).target();
    let mut discovery = BlockDiscovery::new(target, |address: CodeAddress, bytes: &mut [u8]| {
        assert_ne!(address.eip(), 0x2000, "the callee is a separate function");
        read(address, bytes)
    });
    discovery
        .add_root(root, Bits32, DiscoveryLimits::default())
        .unwrap();
    let blocks = discovery.finish().unwrap();
    assert_eq!(
        spans(&blocks),
        [
            (0x1000, 1),
            (0x1001, 3),
            (0x1004, 5),
            (0x1009, 2),
            (0x100b, 1)
        ]
    );
    assert_eq!(exports(&blocks), [0x1000, 0x1009, 0x100b]);
}

#[test]
fn unreadable_and_unsupported_call_continuations_have_no_export() {
    for suffix in [&[][..], &[0xf1]] {
        let mut code = vec![0xff, 0xd0]; // CALL EAX
        code.extend_from_slice(suffix);
        let blocks = discover(
            SegmentProfile::Flat32.into(),
            Bits32,
            &code,
            DiscoveryLimits::default(),
        )
        .unwrap();
        assert_eq!(spans(&blocks), [(START, 2)]);
        assert_eq!(exports(&blocks), [START]);
    }
}

#[test]
fn real_mode_far_calls_export_their_continuations_protected_far_calls_stop() {
    for (profile, call) in [
        (ExecutionProfile::Real16, &[0x9a, 0, 0x20, 0, 0x30][..]),
        (ExecutionProfile::Real16, &[0xff, 0x1e, 0, 0x20]),
        (
            SegmentProfile::Segmented16.into(),
            &[0x9a, 0, 0x20, 0, 0x30],
        ),
        (SegmentProfile::Segmented16.into(), &[0xff, 0x1e, 0, 0x20]),
    ] {
        let mut code = call.to_vec();
        code.extend([0x40, 0xcb, 0x90]); // INC AX; RETF; unused NOP
        let blocks = discover(profile, Bits16, &code, DiscoveryLimits::default()).unwrap();
        let next = START + call.len() as u32;
        if profile == ExecutionProfile::Real16 {
            assert_eq!(spans(&blocks), [(START, call.len()), (next, 2)]);
            assert_eq!(exports(&blocks), [START, next]);
        } else {
            assert_eq!(spans(&blocks), [(START, call.len())]);
            assert_eq!(exports(&blocks), [START]);
        }
    }
}

#[test]
fn requesting_an_internal_instruction_promotes_it_to_a_cpu_entry() {
    let root = address(0x100);
    let second = address(0x110);
    let tail = address(0x120);
    let mut discovery = BlockDiscovery::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        mapped_code([
            (root, &[0xeb, 0x1e][..]), // JMP 0120
            (second, &[0xeb, 0x0e]),   // JMP 0120
            (tail, &[0x90, 0x90, 0xc3]),
        ]),
    );
    for entry in [root, second, address(0x121)] {
        discovery
            .add_root(entry, Bits16, DiscoveryLimits::default())
            .unwrap();
    }
    let blocks = discovery.finish().unwrap();
    assert_eq!(
        spans(&blocks),
        [(0x100, 2), (0x110, 2), (0x120, 1), (0x121, 2)]
    );
    assert_eq!(exports(&blocks), [0x100, 0x110, 0x121]);
}

#[test]
fn interpreted_straight_line_code_can_dispatch_to_either_branch_destination() {
    let profile = ExecutionProfile::Real16;
    let code = [
        0xb0, 0x42, // MOV AL,42h; patched immediate
        0x90, 0x90, // NOP; NOP
        0x75, 0x01, // JNZ 1007
        0xc3, 0xc3, // RET on each path
    ];
    let blocks = discover(
        profile,
        Bits16,
        &code,
        DiscoveryLimits {
            block_instructions: 1,
            ..DiscoveryLimits::default()
        },
    )
    .unwrap();
    let blocks = crate::compiler::stable::stable_blocks(
        Compiler::new(profile).target(),
        blocks,
        |address, len| (address.eip()..address.eip() + len as u32).contains(&(START + 1)),
    )
    .unwrap();
    assert_eq!(exports(&blocks), [0x1002, 0x1006, 0x1007]);
}
