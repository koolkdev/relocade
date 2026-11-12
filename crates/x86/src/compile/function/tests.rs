use super::*;
use crate::{
    decode::snapshot, BlockSnapshot, ExecutionProfile, SegmentDefaultSize, SegmentProfile,
};

fn reader(start: u32, code: &[u8]) -> impl FnMut(u32, &mut [u8]) -> usize + '_ {
    move |eip, output| {
        let offset = eip.wrapping_sub(start) as usize;
        let Some(bytes) = code.get(offset..) else {
            return 0;
        };
        let len = bytes.len().min(output.len());
        output[..len].copy_from_slice(&bytes[..len]);
        len
    }
}

fn entries(code: &[u8], limits: FunctionLimits) -> Vec<(u32, usize)> {
    discover::blocks(
        0x1000,
        Compiler::new(SegmentProfile::Flat32).target(),
        limits,
        reader(0x1000, code),
    )
    .unwrap()
    .into_iter()
    .map(|block| (block.entry.start_eip, block.entry.byte_len))
    .collect()
}

#[test]
fn branches_split_prior_instructions_and_calls_follow_only_continuations() {
    let code = [
        0x90, 0x90, 0x75, 0xfd, // NOP; loop at second NOP
        0xe8, 0xf7, 0x0f, 0, 0, // CALL 2000, outside this readable range
        0xff, 0xd0, // CALL EAX
        0xc3, 0x0f, // RET; incomplete suffix must not be decoded
    ];
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let actual = compiler
        .compile_function(0x1000, FunctionLimits::default(), reader(0x1000, &code))
        .unwrap();
    let snapshots: Vec<_> = [(0, 1), (1, 3), (4, 5), (9, 2), (11, 1)]
        .into_iter()
        .map(|(offset, len)| BlockSnapshot {
            start_eip: 0x1000 + offset as u32,
            bytes: &code[offset..offset + len],
            instruction_limit: 10,
        })
        .collect();
    let expected = compiler.compile_blocks(&snapshots).unwrap();
    assert_eq!(actual.entries, expected.entries);
    assert_eq!(actual.bytes, expected.bytes);
}

#[test]
fn successor_metadata_obeys_operand_width_and_all_near_branch_forms() {
    let target = Compiler::new(SegmentProfile::Flat32).target();
    for code in [
        &[0x75, 0xfe][..],
        &[0xe3, 0xfe],
        &[0xe2, 0xfe],
        &[0xe1, 0xfe],
        &[0xe0, 0xfe],
        &[0x0f, 0x85, 0xfa, 0xff, 0xff, 0xff],
    ] {
        let (instruction, _) = snapshot(code, 0x1000, SegmentDefaultSize::Bits32).unwrap();
        assert_eq!(
            instruction.successors(target),
            [Some(0x1000), Some(0x1000 + code.len() as u32)]
        );
    }
    for (code, expected) in [
        (&[0x66, 0xeb, 0xff][..], [Some(1), None]),
        (&[0x66, 0xe9, 0xfe, 0xff][..], [Some(1), None]),
        (&[0x66, 0xe8, 0x10, 0][..], [Some(3), None]),
        (&[0x66, 0xff, 0xd0][..], [Some(2), None]),
        (&[0xff, 0xe0][..], [None, None]),
        (&[0xc3][..], [None, None]),
    ] {
        let (instruction, _) = snapshot(code, 0xffff, SegmentDefaultSize::Bits32).unwrap();
        assert_eq!(instruction.successors(target), expected, "{code:02x?}");
    }
    let (instruction, _) = snapshot(&[0x66, 0xeb, 0], 0xffff, SegmentDefaultSize::Bits16).unwrap();
    assert_eq!(
        instruction.successors(Compiler::new(ExecutionProfile::Real16).target()),
        [Some(0x10002), None]
    );
}

#[test]
fn limits_apply_to_the_whole_candidate_and_leave_complete_prefixes() {
    let limits = FunctionLimits::default();
    assert_eq!(
        entries(
            &[0x90, 0x90, 0xc3],
            FunctionLimits {
                instructions: 2,
                ..limits
            }
        ),
        [(0x1000, 2)]
    );
    assert!(entries(
        &[0xb8, 1, 0, 0, 0, 0xc3],
        FunctionLimits { bytes: 4, ..limits }
    )
    .is_empty());
    assert_eq!(
        entries(
            &[0xb8, 1, 0, 0, 0, 0xc3],
            FunctionLimits { bytes: 5, ..limits }
        ),
        [(0x1000, 5)]
    );
    let branching = [0x75, 1, 0xc3, 0xc3];
    assert_eq!(
        entries(
            &branching,
            FunctionLimits {
                blocks: 1,
                ..limits
            }
        ),
        [(0x1000, 2)]
    );
    assert_eq!(
        entries(
            &branching,
            FunctionLimits {
                instructions: 2,
                ..limits
            }
        ),
        [(0x1000, 2), (0x1003, 1)]
    );
    assert_eq!(
        entries(&branching, FunctionLimits { bytes: 3, ..limits }),
        [(0x1000, 2), (0x1003, 1)]
    );
    for limits in [
        FunctionLimits {
            instructions: 0,
            ..limits
        },
        FunctionLimits { bytes: 0, ..limits },
        FunctionLimits {
            blocks: 0,
            ..limits
        },
    ] {
        assert!(matches!(
            discover::blocks(
                0,
                Compiler::new(SegmentProfile::Flat32).target(),
                limits,
                |_, _| panic!("zero bounds must not read")
            ),
            Err(BlockError::ZeroDiscoveryLimit)
        ));
    }
}

#[test]
fn unknown_targets_and_overlapping_encodings_end_only_the_affected_path() {
    let limits = FunctionLimits::default();
    assert_eq!(
        entries(&[0x75, 0x7f, 0xc3], limits),
        [(0x1000, 2), (0x1002, 1)]
    );
    // The backward target is inside MOV's immediate.
    assert_eq!(
        entries(&[0xb8, 0x90, 0x90, 0x90, 0x90, 0xeb, 0xfb], limits),
        [(0x1000, 7)]
    );
    // The taken path is decoded first; the fallthrough MOV would overlap it.
    assert_eq!(
        entries(&[0x75, 1, 0xb8, 0x90, 0x90, 0x90, 0xc3], limits),
        [(0x1000, 2), (0x1003, 4)]
    );
    for code in [&[0x90, 0xb8][..], &[0x90, 0x0f, 0xff]] {
        assert!(Compiler::new(SegmentProfile::Flat32)
            .compile_function(0x1000, limits, reader(0x1000, code))
            .is_err());
    }
}

#[test]
fn short_reads_and_address_wrap_reuse_bytes_without_redecoding() {
    let mut code = vec![0x90; 260];
    code.push(0xc3);
    let mut copied = 0;
    let mut source = reader(u32::MAX - 10, &code);
    let blocks = discover::blocks(
        u32::MAX - 10,
        Compiler::new(SegmentProfile::Flat32).target(),
        FunctionLimits::default(),
        |eip, output| {
            let len = output.len().min(7);
            let count = source(eip, &mut output[..len]);
            copied += count;
            count
        },
    )
    .unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].entry.byte_len, code.len());
    assert_eq!(blocks[0].instructions.len(), code.len());
    assert_eq!(copied, code.len());
}
