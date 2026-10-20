use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{CallPatches, TestModule},
    },
    BlockEntry, BlockError, BlockSnapshot, Compiler, CpuFeature, CpuModel, CpuState,
    ExecutionContext, ExecutionExit, ExecutionProfile, SegmentProfile, Segments,
};
use wasmparser::{Parser, Payload, Validator};

#[test]
fn entries_report_consumed_spans_in_input_order() {
    let blocks = Compiler::new(SegmentProfile::Flat32)
        .compile_blocks(&[
            BlockSnapshot {
                start_eip: u32::MAX,
                bytes: &[0x90, 0x90, 0x0f],
                instruction_limit: 3,
            },
            BlockSnapshot {
                start_eip: 0x1000,
                bytes: &[0x90, 0x90, 0x0f],
                instruction_limit: 3,
            },
            BlockSnapshot {
                start_eip: 0x1001,
                bytes: &[0x90, 0xeb, 0xfd, 0x0f],
                instruction_limit: 10,
            },
            BlockSnapshot {
                start_eip: 1,
                bytes: &[0x0f, 0x0b, 0x0f],
                instruction_limit: 10,
            },
        ])
        .unwrap();
    assert_eq!(
        blocks.entries,
        [
            BlockEntry {
                start_eip: u32::MAX,
                byte_len: 2,
                export: "block_ffffffff".into()
            },
            BlockEntry {
                start_eip: 0x1000,
                byte_len: 1,
                export: "block_1000".into()
            },
            BlockEntry {
                start_eip: 0x1001,
                byte_len: 3,
                export: "block_1001".into()
            },
            BlockEntry {
                start_eip: 1,
                byte_len: 2,
                export: "block_1".into()
            },
        ]
    );
    Validator::new().validate_all(&blocks.bytes).unwrap();
}

#[test]
fn an_entry_inside_an_encoding_does_not_split_the_instruction() {
    let blocks = Compiler::new(SegmentProfile::Flat32)
        .compile_blocks(&[
            BlockSnapshot {
                start_eip: 0x1000,
                bytes: &[0xb8, 0x90, 0x90, 0x90, 0x90],
                instruction_limit: 1,
            },
            BlockSnapshot {
                start_eip: 0x1002,
                bytes: &[0x90],
                instruction_limit: 1,
            },
        ])
        .unwrap();
    assert_eq!(blocks.entries[0].byte_len, 5);
    assert_eq!(blocks.entries[1].byte_len, 1);
}

#[test]
fn empty_input_is_valid_and_invalid_snapshots_reject_the_module() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let empty = compiler.compile_blocks(&[]).unwrap();
    assert!(empty.entries.is_empty());
    assert_eq!(empty.execution_target, compiler.target());
    Validator::new().validate_all(&empty.bytes).unwrap();

    let first = BlockSnapshot {
        start_eip: 0x1000,
        bytes: &[0x90],
        instruction_limit: 1,
    };
    assert_eq!(
        compiler.compile_blocks(&[first, first]).err(),
        Some(BlockError::DuplicateBlock { address: 0x1000 })
    );
    assert_eq!(
        compiler
            .compile_blocks(&[
                first,
                BlockSnapshot {
                    start_eip: 0x2000,
                    bytes: &[0xb8],
                    instruction_limit: 1,
                }
            ])
            .err(),
        Some(BlockError::TruncatedInstruction {
            address: 0x2000,
            available: 1
        })
    );
    assert_eq!(
        compiler
            .compile_blocks(&[BlockSnapshot {
                instruction_limit: 0,
                ..first
            }])
            .err(),
        Some(BlockError::ZeroInstructionLimit)
    );
}

fn entry_boundaries() {
    let compiler = Compiler::new(ExecutionProfile::Real16)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false));
    let snapshots = [
        BlockSnapshot {
            start_eip: 0x1000,
            bytes: &[0xb8, 0x34, 0x12, 0xbb, 0x78, 0x56], // MOV AX,1234; MOV BX,5678
            instruction_limit: 2,
        },
        BlockSnapshot {
            start_eip: 0x1003,
            bytes: &[0xbb, 0x78, 0x56],
            instruction_limit: 1,
        },
        BlockSnapshot {
            start_eip: 0x2000,
            bytes: &[0x0f, 0x44, 0xc0],
            instruction_limit: 1,
        },
        BlockSnapshot {
            start_eip: 0xffff,
            bytes: &[0x89, 0xc0],
            instruction_limit: 1,
        },
    ];
    let blocks = compiler.compile_blocks(&snapshots).unwrap();
    assert_eq!(blocks.execution_target, compiler.target());
    let mut module = TestModule::from_bytes(
        &blocks.bytes,
        &blocks.entries[0].export,
        Some(blocks.execution_target),
    );
    for (index, (snapshot, entry)) in snapshots.iter().zip(&blocks.entries).enumerate() {
        module.entry.clone_from(&entry.export);
        let mut image = Image::empty();
        image.cpu = CpuState {
            eip: snapshot.start_eip,
            instruction_count: 17,
            segments: Segments::real_mode(),
            ..CpuState::default()
        };
        image.map(snapshot.start_eip >> 12, 0x3000, false);
        image.map((snapshot.start_eip >> 12) + 1, 0x4000, false);
        image.data(0x3000 + (snapshot.start_eip & 0xfff), snapshot.bytes);
        let mut after = image.cpu;
        let exit = match index {
            0 => {
                after.registers.eax = 0x1234;
                after.registers.ebx = 0x5678;
                after.eip = 0x1006;
                after.instruction_count = 19;
                Exit::Dispatch(0x1006)
            }
            1 => {
                after.registers.ebx = 0x5678;
                after.eip = 0x1006;
                after.instruction_count = 18;
                Exit::Dispatch(0x1006)
            }
            2 => Exit::InvalidOpcode,
            3 => Exit::Interpret,
            _ => unreachable!(),
        };
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[],
                    exit
                }]
            ),
            "{}",
            entry.export,
        );
        if index == 0 {
            after.registers.ebx = image.cpu.registers.ebx;
            after.eip = 0x1003;
            after.instruction_count = 18;
            let mut input = image.input();
            input.patches_before_calls.push(CallPatches::cpu(vec![(
                std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
                18_u64.to_le_bytes().to_vec(),
            )]));
            assert_eq!(
                module.observe(&input, 1),
                expected(
                    &image,
                    &[Step {
                        cpu: after,
                        ram: &[],
                        exit: Exit::Other(ExecutionExit::BudgetExhausted.to_bits()),
                    }]
                ),
            );
        }
    }
}

fn function_count(bytes: &[u8]) -> u32 {
    Parser::new(0)
        .parse_all(bytes)
        .find_map(|payload| match payload.unwrap() {
            Payload::CodeSectionStart { count, .. } => Some(count),
            _ => None,
        })
        .unwrap()
}

fn shared_memory_helpers() {
    for profile in [SegmentProfile::Flat32.into(), ExecutionProfile::Real16] {
        let compiler = Compiler::new(profile);
        // MOV EAX,[ECX]; MOV [EBX],EAX, with 32-bit operands and addresses in both modes.
        let code: &[u8] = if profile == ExecutionProfile::Real16 {
            &[0x66, 0x67, 0x8b, 0x01, 0x66, 0x67, 0x89, 0x03]
        } else {
            &[0x8b, 0x01, 0x89, 0x03]
        };
        let blocks = compiler
            .compile_blocks(&[
                BlockSnapshot {
                    start_eip: 0x1000,
                    bytes: code,
                    instruction_limit: 2,
                },
                BlockSnapshot {
                    start_eip: 0x1100,
                    bytes: code,
                    instruction_limit: 2,
                },
            ])
            .unwrap();
        let single = compiler.compile_block(0x1000, code, 2).unwrap();
        // A second body reuses every memory helper already needed by the first.
        assert_eq!(
            function_count(&blocks.bytes),
            function_count(&single.bytes) + 1
        );
        let mut module = TestModule::from_bytes(
            &blocks.bytes,
            &blocks.entries[0].export,
            Some(blocks.execution_target),
        );
        for entry in &blocks.entries {
            module.entry.clone_from(&entry.export);
            let mut image = Image::empty();
            image.cpu.instruction_count = 40;
            image.cpu.eip = entry.start_eip;
            image.cpu.registers.ecx = 0x2fff;
            image.cpu.registers.ebx = 0x4fff;
            if profile == ExecutionProfile::Real16 {
                image.cpu.segments = Segments::real_mode();
            }
            image.map(1, 0x1000, false);
            image.data(entry.start_eip, code);
            for (page, frame) in [(2, 0x4000), (3, 0x6000), (4, 0x8000), (5, 0xa000)] {
                image.map(page, frame, true);
            }
            image.data(0x4fff, &[0x11]);
            image.data(0x6000, &[0x22, 0x33, 0x44]);
            let mut after = image.cpu;
            after.registers.eax = 0x4433_2211;
            after.eip += code.len() as u32;
            after.instruction_count = 42;
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: after,
                        ram: &[(0x8fff, &[0x11]), (0xa000, &[0x22, 0x33, 0x44])],
                        exit: Exit::Dispatch(after.eip),
                    }]
                ),
                "{profile:?}, {}",
                entry.export,
            );
        }
    }
}

#[test]
fn exports_preserve_execution_boundaries() {
    entry_boundaries();
}

#[test]
fn entries_share_memory_helpers() {
    shared_memory_helpers();
}
