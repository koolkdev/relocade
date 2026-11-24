use super::*;

#[test]
fn per_root_budgets_count_new_instructions_and_their_complete_encodings() {
    let limits = DiscoveryLimits::default();
    let profile = SegmentProfile::Flat32.into();
    for (code, limits) in [
        (
            &[0x90, 0x90, 0xc3][..],
            DiscoveryLimits {
                instructions: 2,
                ..limits
            },
        ),
        (
            &[0xb8, 1, 0, 0, 0, 0xc3],
            DiscoveryLimits { bytes: 4, ..limits },
        ),
        (
            &[0xb8, 1, 0, 0, 0, 0xc3],
            DiscoveryLimits { bytes: 5, ..limits },
        ),
        (
            &[0x75, 1, 0xc3, 0xc3],
            DiscoveryLimits {
                instructions: 2,
                ..limits
            },
        ),
        (
            &[0x75, 1, 0xc3, 0xc3],
            DiscoveryLimits { bytes: 3, ..limits },
        ),
    ] {
        assert!(
            matches!(
                discover(profile, Bits32, code, limits),
                Err(Error::Discovery(DiscoveryError::TooLarge {
                    address: START
                }))
            ),
            "{code:02x?}, {limits:?}"
        );
    }

    let blocks = discover(
        profile,
        Bits32,
        &[0x75, 0xfe, 0xc3], // JNZ back to the root; RET
        DiscoveryLimits {
            instructions: 2,
            bytes: 3,
            ..limits
        },
    )
    .unwrap();
    assert_eq!(spans(&blocks), [(START, 2), (START + 2, 1)]);
}

#[test]
fn zero_limits_are_rejected_before_reading_the_source() {
    let limits = DiscoveryLimits::default();
    let target = Compiler::new(SegmentProfile::Flat32).config();
    for limits in [
        DiscoveryLimits {
            instructions: 0,
            ..limits
        },
        DiscoveryLimits { bytes: 0, ..limits },
        DiscoveryLimits {
            block_instructions: 0,
            ..limits
        },
    ] {
        let mut discovery = BlockDiscovery::new(target, |_, _: &mut [u8]| {
            panic!("attempted to read code for a zero limit")
        });
        assert!(matches!(
            discovery.add_root(address(START), Bits32, limits),
            Err(Error::Discovery(DiscoveryError::ZeroLimit))
        ));
    }
}

#[test]
fn default_limits_accept_a_large_branch_chain() {
    let mut code: Vec<_> = std::iter::repeat_n([0x75, 0], 8192).flatten().collect();
    code.push(0xc3);
    let blocks = discover(
        SegmentProfile::Flat32.into(),
        Bits32,
        &code,
        DiscoveryLimits::default(),
    )
    .unwrap();
    assert_eq!(blocks.len(), 8193);
    assert_eq!(
        blocks.iter().map(DecodedBlock::byte_len).sum::<usize>(),
        code.len()
    );
    assert_eq!(blocks.last().unwrap().address(), address(0x5000));
}

#[test]
fn short_sequential_reads_reuse_bytes_across_an_eip_wrap() {
    let start = address(u32::MAX - 10);
    let mut code = vec![0x90; 260];
    code.push(0xc3);
    let mut copied = 0;
    let mut read = mapped_code([(start, code.as_slice())]);
    let target = Compiler::new(SegmentProfile::Flat32).config();
    let mut discovery = BlockDiscovery::new(target, |address, output: &mut [u8]| {
        let len = output.len().min(7);
        let count = read(address, &mut output[..len]);
        copied += count;
        count
    });
    discovery
        .add_root(start, Bits32, DiscoveryLimits::default())
        .unwrap();
    let blocks = discovery.finish().unwrap();
    assert_eq!(spans(&blocks), [(245, 5), (u32::MAX - 10, 256)]);
    assert_eq!(copied, code.len());
}
