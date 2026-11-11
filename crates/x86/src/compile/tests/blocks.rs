use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{CallPatches, TestModule},
    },
    BlockCompileError, BlockError, BlockSnapshot, Compiler, CpuFeature, CpuModel, CpuState,
    ExecutionContext, ExecutionExit, ExecutionProfile, SegmentProfile,
};
use wasmparser::{Parser, Payload, Validator};

#[test]
fn entries_report_consumed_spans_in_input_order() {
    let snapshots = [
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
    ];
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let blocks = compiler.compile_blocks(&snapshots).unwrap();
    assert_eq!(
        blocks
            .entries
            .iter()
            .map(|entry| (entry.start_eip, entry.byte_len, entry.export.as_str()))
            .collect::<Vec<_>>(),
        [
            (u32::MAX, 2, "block_ffffffff"),
            (0x1000, 1, "block_1000"),
            (0x1001, 3, "block_1001"),
            (1, 2, "block_1"),
        ]
    );
    Validator::new().validate_all(&blocks.bytes).unwrap();
    let parallel = parallel_blocks(&compiler, &snapshots);
    assert_eq!(parallel.bytes, blocks.bytes);
    assert_eq!(parallel.entries, blocks.entries);
    assert_eq!(parallel.execution_target, blocks.execution_target);
}

fn parallel_blocks(compiler: &Compiler, snapshots: &[BlockSnapshot<'_>]) -> crate::CompiledBlocks {
    let (pool, jobs) = relocade_workers::Pool::new(vec![
        relocade_workers::Thread::new(),
        relocade_workers::Thread::new(),
    ])
    .unwrap();
    let ((), parallel) = pollster::block_on(futures_util::future::join(pool.run(), async move {
        compiler
            .compile_blocks_with(snapshots, &jobs)
            .await
            .unwrap()
    }));
    parallel
}

#[test]
fn entries_record_consumed_segment_facts_and_decode_defaults() {
    use crate::{CpuMode, EntryFact, Segment, SegmentDefaultSize, SegmentFact};
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let snapshots = [
        BlockSnapshot {
            start_eip: 0x1000,
            bytes: &[0x90],
            instruction_limit: 1,
        },
        BlockSnapshot {
            start_eip: 0x2000,
            bytes: &[0x8b, 0x00],
            instruction_limit: 1,
        }, // MOV EAX,[EAX]
        BlockSnapshot {
            start_eip: 0x3000,
            bytes: &[0x64, 0x8b, 0x00],
            instruction_limit: 1,
        }, // MOV EAX,FS:[EAX]
    ];
    let blocks = compiler.compile_blocks(&snapshots).unwrap();
    let parallel = parallel_blocks(&compiler, &snapshots);
    assert_eq!(parallel.entries, blocks.entries);
    assert_eq!(parallel.bytes, blocks.bytes);
    for entry in &blocks.entries {
        assert!(entry
            .requirements
            .facts()
            .contains(&EntryFact::Mode(CpuMode::Protected)));
        assert!(entry
            .requirements
            .facts()
            .contains(&EntryFact::CodeDefaultSize(SegmentDefaultSize::Bits32)));
        assert!(entry.requirements.matches(&CpuState::default()));
        assert!(!entry.requirements.facts().iter().any(|fact| matches!(
            fact,
            EntryFact::Segment {
                segment: Segment::Fs,
                ..
            }
        )));
    }
    let ds_base = EntryFact::Segment {
        segment: Segment::Ds,
        fact: SegmentFact::Base(0),
    };
    assert!(!blocks.entries[0].requirements.facts().contains(&ds_base));
    assert!(blocks.entries[1].requirements.facts().contains(&ds_base));
    assert!(!blocks.entries[2].requirements.facts().contains(&ds_base));
    assert!(!blocks.entries[1]
        .requirements
        .facts()
        .contains(&EntryFact::Segment {
            segment: Segment::Ds,
            fact: SegmentFact::DefaultSize(SegmentDefaultSize::Bits32),
        }));
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
fn empty_input_is_valid_and_compilation_errors_are_reported() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let empty = compiler.compile_blocks(&[]).unwrap();
    assert!(empty.entries.is_empty());
    assert!(super::memory::imports(&empty.bytes).is_empty());
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

    let (pool, jobs) = relocade_workers::Pool::new(vec![relocade_workers::Thread::new()]).unwrap();
    drop(pool);
    pollster::block_on(async {
        assert!(matches!(
            compiler.compile_blocks_with(&[first, first], &jobs).await,
            Err(BlockCompileError::Block(BlockError::DuplicateBlock {
                address: 0x1000
            }))
        ));
        assert!(matches!(
            compiler.compile_blocks_with(&[first], &jobs).await,
            Err(BlockCompileError::Execution(
                relocade_workers::PoolError::Closed
            ))
        ));
    });
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
    let mut module = TestModule::from_blocks(&blocks);
    for (index, (snapshot, entry)) in snapshots.iter().zip(&blocks.entries).enumerate() {
        module.select_entry(entry);
        let mut image = Image::empty();
        image.cpu = CpuState {
            eip: snapshot.start_eip,
            instruction_count: 17,
            ..CpuState::real_mode()
        };
        image.map(snapshot.start_eip >> 12, 0x3000, false);
        image.map((snapshot.start_eip >> 12) + 1, 0x4000, false);
        image.data(0x3000 + (snapshot.start_eip & 0xfff), snapshot.bytes);
        if index == 2 {
            image.cpu.registers.esp = 0x8006;
            image.map(0, 0x7000, false);
            image.map(8, 0x8000, true);
            image.data(0x7018, &[0, 6, 0, 0]); // #UD vector -> 0000:0600.
        }
        let mut after = image.cpu;
        let exit = match index {
            0 => {
                after.registers.eax = 0x1234;
                after.eip = 0x1003;
                after.instruction_count = 18;
                Exit::Dispatch(0x1003)
            }
            1 => {
                after.registers.ebx = 0x5678;
                after.eip = 0x1006;
                after.instruction_count = 18;
                Exit::Dispatch(0x1006)
            }
            2 => {
                after.eip = 0x600;
                after.registers.esp = 0x8000;
                Exit::Other(crate::ExecutionExit::InterruptEntered)
            }
            3 => Exit::Interpret,
            _ => unreachable!(),
        };
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: if index == 2 {
                        &[(0x8000, &[0, 0x20, 0, 0, 2, 0])]
                    } else {
                        &[]
                    },
                    exit
                }]
            ),
            "{}",
            entry.export,
        );
        if index == 0 {
            let mut input = image.input();
            input.patches_before_calls.push(CallPatches::cpu(vec![(
                std::mem::offset_of!(ExecutionContext, interrupt_requested) as u32,
                vec![1],
            )]));
            assert_eq!(
                module.observe(&input, 1),
                expected(
                    &image,
                    &[Step {
                        cpu: after,
                        ram: &[],
                        exit: Exit::Other(ExecutionExit::Interrupted),
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
        let mapping = if profile == ExecutionProfile::Real16 {
            "physicalMap"
        } else {
            "machine"
        };
        assert_eq!(
            super::memory::imports(&blocks.bytes),
            vec![
                ("relocade", "executionContext"),
                ("relocade", "guest"),
                ("relocade", mapping),
            ]
        );
        // A second body reuses every memory helper already needed by the first.
        assert_eq!(
            function_count(&blocks.bytes),
            function_count(&single.bytes) + 1
        );
        let mut module = TestModule::from_blocks(&blocks);
        for entry in &blocks.entries {
            module.select_entry(entry);
            let mut image = Image::empty();
            image.cpu.instruction_count = 40;
            image.cpu.eip = entry.start_eip;
            image.cpu.registers.ecx = 0x2fff;
            image.cpu.registers.ebx = 0x4fff;
            if profile == ExecutionProfile::Real16 {
                image.initialize_real_mode();
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
