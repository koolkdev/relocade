mod boundaries;
mod linking;

use super::*;
use crate::{BlockSnapshot, ExecutionProfile, SegmentDefaultSize, SegmentProfile};

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
        CodeAddress::CsRelative(0x1000),
        SegmentDefaultSize::Bits32,
        Compiler::new(SegmentProfile::Flat32).target(),
        limits,
        reader(0x1000, code),
    )
    .unwrap()
    .into_iter()
    .map(|block| (block.entry.address.eip(), block.entry.byte_len))
    .collect()
}

#[test]
fn snapshots_and_discovered_blocks_record_their_decode_width() {
    use SegmentDefaultSize::{Bits16, Bits32};
    let compiler = Compiler::new(SegmentProfile::Segmented32);
    let code = [0xb8, 0x34, 0x12, 0xc3, 0x90, 0xc3];
    let starts = [(0x1000, Bits16), (0x2000, Bits32)];
    let snapshots = starts.map(|(eip, code_default_size)| BlockSnapshot {
        address: CodeAddress::CsRelative(eip),
        code_default_size,
        bytes: &code,
        instruction_limit: 1,
    });
    let mut functions = compiler.function_module();
    for (eip, size) in starts {
        functions
            .add_function(
                CodeAddress::CsRelative(eip),
                size,
                FunctionLimits::default(),
                reader(eip, &code),
            )
            .unwrap();
    }
    let entries = |blocks: crate::CompiledBlocks| {
        blocks
            .entries
            .into_iter()
            .map(|block| (block.address.eip(), block.code_default_size, block.byte_len))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        entries(compiler.compile_blocks(&snapshots).unwrap()),
        [(0x1000, Bits16, 3), (0x2000, Bits32, 5)]
    );
    assert_eq!(
        entries(functions.compile().unwrap()),
        [(0x1000, Bits16, 4), (0x2000, Bits32, 6)]
    );
}

#[test]
fn branches_split_prior_instructions_and_calls_follow_only_continuations() {
    let code = [
        0x90, 0x90, 0x75, 0xfd, // NOP; loop at second NOP
        0xe8, 0xf7, 0x0f, 0, 0, // CALL 2000, outside this readable range
        0xff, 0xd0, // CALL EAX
        0xc3, 0x90, // RET; unused NOP must not be decoded
    ];
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let actual = compiler
        .compile_function(0x1000, FunctionLimits::default(), reader(0x1000, &code))
        .unwrap();
    let snapshots: Vec<_> = [(0, 1), (1, 3), (4, 5), (9, 2), (11, 1)]
        .into_iter()
        .map(|(offset, len)| BlockSnapshot {
            address: CodeAddress::CsRelative(0x1000 + offset as u32),
            code_default_size: SegmentDefaultSize::Bits32,
            bytes: &code[offset..offset + len],
            instruction_limit: 10,
        })
        .collect();
    let expected = compiler.compile_blocks(&snapshots).unwrap();
    assert_eq!(actual.entries, expected.entries);
}

#[test]
fn exploration_caps_reject_the_candidate_and_accept_exact_fits() {
    let limits = FunctionLimits::default();
    let compiler = Compiler::new(SegmentProfile::Flat32);
    for (code, limits) in [
        (
            &[0x90, 0x90, 0xc3][..],
            FunctionLimits {
                instructions: 2,
                ..limits
            },
        ),
        (
            &[0xb8, 1, 0, 0, 0, 0xc3][..],
            FunctionLimits { bytes: 4, ..limits },
        ),
        (
            &[0xb8, 1, 0, 0, 0, 0xc3][..],
            FunctionLimits { bytes: 5, ..limits },
        ),
        (
            &[0x75, 1, 0xc3, 0xc3][..],
            FunctionLimits {
                instructions: 2,
                ..limits
            },
        ),
        (
            &[0x75, 1, 0xc3, 0xc3][..],
            FunctionLimits { bytes: 3, ..limits },
        ),
    ] {
        assert_eq!(
            compiler
                .compile_function(0x1000, limits, reader(0x1000, code))
                .err(),
            Some(BlockError::FunctionTooLarge { address: 0x1000 }),
        );
    }
    assert_eq!(
        entries(
            &[0x90, 0x90, 0xc3],
            FunctionLimits {
                instructions: 3,
                bytes: 3,
                ..limits
            }
        ),
        [(0x1000, 3)]
    );
    // Reaching an already decoded loop entry does not consume more exploration budget.
    assert_eq!(
        entries(
            &[0x75, 0xfe, 0xc3],
            FunctionLimits {
                instructions: 2,
                bytes: 3,
                ..limits
            }
        ),
        [(0x1000, 2), (0x1002, 1)]
    );
    for limits in [
        FunctionLimits {
            instructions: 0,
            ..limits
        },
        FunctionLimits { bytes: 0, ..limits },
        FunctionLimits {
            block_instructions: 0,
            ..limits
        },
    ] {
        let result = discover::blocks(
            CodeAddress::CsRelative(0),
            SegmentDefaultSize::Bits32,
            compiler.target(),
            limits,
            |_, _| panic!("zero bounds must not read"),
        );
        assert!(matches!(result, Err(BlockError::ZeroFunctionLimit)));
    }
}

#[test]
fn straight_line_splits_preserve_the_whole_function_and_instruction_boundaries() {
    let code = [0xb8, 1, 0, 0, 0, 0x90, 0x90, 0x90, 0xc3];
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let actual = compiler
        .compile_function(
            0x1000,
            FunctionLimits {
                block_instructions: 2,
                ..FunctionLimits::default()
            },
            reader(0x1000, &code),
        )
        .unwrap();
    let snapshots: Vec<_> = [(0, 6), (6, 2), (8, 1)]
        .into_iter()
        .map(|(offset, len)| BlockSnapshot {
            address: CodeAddress::CsRelative(0x1000 + offset as u32),
            code_default_size: SegmentDefaultSize::Bits32,
            bytes: &code[offset..offset + len],
            instruction_limit: 2,
        })
        .collect();
    let expected = compiler.compile_blocks(&snapshots).unwrap();
    assert_eq!(actual.entries, expected.entries);
}

#[test]
fn defaults_discover_large_branched_functions_in_full() {
    let mut code: Vec<_> = std::iter::repeat_n([0x75, 0], 8192).flatten().collect();
    code.push(0xc3);
    let blocks = entries(&code, FunctionLimits::default());
    assert_eq!(blocks.len(), 8193);
    assert_eq!(blocks.iter().map(|(_, len)| len).sum::<usize>(), code.len());
    assert_eq!(blocks.last(), Some(&(0x5000, 1)));
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
}

#[test]
fn short_reads_and_address_wrap_reuse_bytes_without_redecoding() {
    let mut code = vec![0x90; 260];
    code.push(0xc3);
    let mut copied = 0;
    let mut source = reader(u32::MAX - 10, &code);
    let blocks = discover::blocks(
        CodeAddress::CsRelative(u32::MAX - 10),
        SegmentDefaultSize::Bits32,
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
    assert_eq!(
        blocks
            .iter()
            .map(|block| (block.entry.address.eip(), block.entry.byte_len))
            .collect::<Vec<_>>(),
        [(245, 5), (u32::MAX - 10, 256)],
    );
    assert_eq!(
        blocks
            .iter()
            .map(|block| block.instructions.len())
            .sum::<usize>(),
        code.len()
    );
    assert_eq!(copied, code.len());
}
