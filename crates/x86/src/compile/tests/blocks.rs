use crate::{
    compile::block::module::BlockModule,
    profile::EntryGuarantees,
    support::{
        machine::{expected, Exit, Image, Step},
        step::{CallPatches, TestModule},
    },
    BlockCompileError, BlockEntry, BlockError, BlockSnapshot, CodeAddress, CompiledBlocks,
    Compiler, CpuFeature, CpuModel, CpuState, ExecutionContext, ExecutionExit, ExecutionProfile,
    SegmentDefaultSize, SegmentProfile,
};
use wasmparser::{Parser, Payload, Validator};

#[test]
fn entries_report_consumed_spans_in_input_order() {
    let code_default_size = SegmentDefaultSize::Bits32;
    let snapshots = [
        BlockSnapshot {
            address: CodeAddress::CsRelative(u32::MAX),
            code_default_size,
            bytes: &[0x90, 0x90, 0x0f],
            instruction_limit: 3,
        },
        BlockSnapshot {
            address: CodeAddress::CsRelative(0x1000),
            code_default_size,
            bytes: &[0x90, 0x90, 0x0f],
            instruction_limit: 3,
        },
        BlockSnapshot {
            address: CodeAddress::CsRelative(0x1001),
            code_default_size,
            bytes: &[0x90, 0xeb, 0xfd, 0x0f],
            instruction_limit: 10,
        },
        BlockSnapshot {
            address: CodeAddress::CsRelative(1),
            code_default_size,
            bytes: &[0x0f, 0x0b, 0x0f],
            instruction_limit: 10,
        },
    ];
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let blocks = compiler.compile_blocks(&snapshots).unwrap();
    assert_eq!(
        blocks.entries,
        [
            BlockEntry {
                address: CodeAddress::CsRelative(u32::MAX),
                code_default_size,
                byte_len: 2,
                export: "block_ffffffff".into()
            },
            BlockEntry {
                address: CodeAddress::CsRelative(0x1000),
                code_default_size,
                byte_len: 1,
                export: "block_1000".into()
            },
            BlockEntry {
                address: CodeAddress::CsRelative(0x1001),
                code_default_size,
                byte_len: 3,
                export: "block_1001".into()
            },
            BlockEntry {
                address: CodeAddress::CsRelative(1),
                code_default_size,
                byte_len: 2,
                export: "block_1".into()
            },
        ]
    );
    Validator::new().validate_all(&blocks.bytes).unwrap();
    let (pool, jobs) = relocade_workers::Pool::new(vec![
        relocade_workers::Thread::new(),
        relocade_workers::Thread::new(),
    ])
    .unwrap();
    let ((), parallel) = pollster::block_on(futures_util::future::join(pool.run(), async move {
        compiler
            .compile_blocks_with(&snapshots, &jobs)
            .await
            .unwrap()
    }));
    assert_eq!(parallel.bytes, blocks.bytes);
    assert_eq!(parallel.entries, blocks.entries);
    assert_eq!(parallel.execution_target, blocks.execution_target);
}

fn compile_without_code_default(
    compiler: &Compiler,
    snapshot: BlockSnapshot<'_>,
) -> CompiledBlocks {
    let guarantees = EntryGuarantees::for_profile(compiler.target().profile).without_code_default();
    BlockModule::with_entry_guarantees(compiler, guarantees)
        .build(&[snapshot])
        .unwrap()
        .compile()
        .unwrap()
}

fn check_width_guard(blocks: &CompiledBlocks, needs_guard: bool) {
    // This register-only MOV accesses just the CPU state memory.
    assert_eq!(
        super::memory::imports(&blocks.bytes),
        [("relocade", "executionContext")]
    );
    let cs_big = std::mem::offset_of!(ExecutionContext, cpu.segments.cs.big) as u64;
    let mut loads_width = false;
    let mut imports_interpreter = false;
    for payload in Parser::new(0).parse_all(&blocks.bytes) {
        match payload.unwrap() {
            Payload::ImportSection(imports) => {
                imports_interpreter = imports.into_iter().any(|import| {
                    let import = import.unwrap();
                    (import.module, import.name) == ("relocade", crate::abi::INTERPRET.name)
                });
            }
            Payload::CodeSectionEntry(body) => {
                loads_width |= body.get_operators_reader().unwrap().into_iter().any(|op| {
                    matches!(op.unwrap(), wasmparser::Operator::I32Load8U { memarg } if memarg.offset == cs_big)
                });
            }
            _ => {}
        }
    }
    assert_eq!(loads_width, needs_guard, "CS.D load");
    assert_eq!(imports_interpreter, needs_guard, "interpreter fallback");
}

#[test]
fn the_target_code_width_folds_the_destination_guard() {
    let size = SegmentDefaultSize::Bits16;
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let bytes = [0xb8, 0x34, 0x12];
    let snapshot = BlockSnapshot {
        address: CodeAddress::CsRelative(0x1000),
        code_default_size: size,
        bytes: &bytes,
        instruction_limit: 1,
    };
    let batch = compiler.compile_blocks(&[snapshot]).unwrap();
    let fixed = compiler.compile_block(0x1000, &bytes, 1).unwrap();
    assert_eq!(batch.bytes, fixed.bytes);
    check_width_guard(&batch, false);
    check_width_guard(&compile_without_code_default(&compiler, snapshot), true);
}

#[test]
fn a_width_mismatch_hands_off_before_executing_snapshot_bytes() {
    use SegmentDefaultSize::{Bits16, Bits32};
    let code = [0xb8, 0x34, 0x12, 0x78, 0x56]; // MOV AX/EAX, immediate
    let compiler = Compiler::new(SegmentProfile::Segmented32);
    for (required, result, next_eip) in
        [(Bits16, 0xaaaa_1234, 0x1003), (Bits32, 0x5678_1234, 0x1005)]
    {
        let blocks = compile_without_code_default(
            &compiler,
            BlockSnapshot {
                address: CodeAddress::CsRelative(0x1000),
                code_default_size: required,
                bytes: &code,
                instruction_limit: 1,
            },
        );
        // The same destination must work for either live default.
        let module = TestModule::from_bytes(&blocks.bytes, &blocks.entries[0].export, None);
        for big in [0, 1, 0x80] {
            let mut image = Image::new(&code);
            image.cpu = CpuState::protected_mode(Bits32, Bits32);
            image.cpu.segments.cs.big = big;
            image.cpu.eip = 0x1000;
            image.cpu.registers.eax = 0xaaaa_aaaa;
            let mut after = image.cpu;
            let exit = if image.cpu.code_default_size() == required {
                after.eip = next_eip;
                after.registers.eax = result;
                after.instruction_count += 1;
                Exit::Dispatch(next_eip)
            } else {
                Exit::Interpret
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
                )
            );
        }
    }
}

#[test]
fn real_mode_entries_split_only_at_boundaries_in_their_own_segment() {
    let snapshots = [(0x10, 0), (0x20, 0), (0x20, 1)].map(|(segment, eip)| BlockSnapshot {
        address: CodeAddress::RealMode { segment, eip },
        code_default_size: SegmentDefaultSize::Bits16,
        bytes: &[0x90, 0x90],
        instruction_limit: 2,
    });
    let blocks = Compiler::new(ExecutionProfile::Real16)
        .compile_blocks(&snapshots)
        .unwrap();
    let entries: Vec<_> = blocks
        .entries
        .iter()
        .map(|block| (block.export.as_str(), block.byte_len))
        .collect();
    assert_eq!(
        entries,
        [
            ("block_0010:0000", 2),
            ("block_0020:0000", 1),
            ("block_0020:0001", 2)
        ]
    );
}

#[test]
fn an_entry_inside_an_encoding_does_not_split_the_instruction() {
    let code_default_size = SegmentDefaultSize::Bits32;
    let blocks = Compiler::new(SegmentProfile::Flat32)
        .compile_blocks(&[
            BlockSnapshot {
                address: CodeAddress::CsRelative(0x1000),
                code_default_size,
                bytes: &[0xb8, 0x90, 0x90, 0x90, 0x90],
                instruction_limit: 1,
            },
            BlockSnapshot {
                address: CodeAddress::CsRelative(0x1002),
                code_default_size,
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
    let code_default_size = SegmentDefaultSize::Bits32;
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let empty = compiler.compile_blocks(&[]).unwrap();
    assert!(empty.entries.is_empty());
    assert!(super::memory::imports(&empty.bytes).is_empty());
    assert_eq!(empty.execution_target, compiler.target());
    Validator::new().validate_all(&empty.bytes).unwrap();

    let first = BlockSnapshot {
        address: CodeAddress::CsRelative(0x1000),
        code_default_size,
        bytes: &[0x90],
        instruction_limit: 1,
    };
    assert_eq!(
        compiler.compile_blocks(&[first, first]).err(),
        Some(BlockError::DuplicateBlock {
            address: CodeAddress::CsRelative(0x1000)
        })
    );
    assert_eq!(
        compiler
            .compile_blocks(&[
                first,
                BlockSnapshot {
                    address: CodeAddress::CsRelative(0x2000),
                    code_default_size,
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
                address: CodeAddress::CsRelative(0x1000)
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
    let code_default_size = SegmentDefaultSize::Bits16;
    let snapshots = [
        BlockSnapshot {
            address: CodeAddress::CsRelative(0x1000),
            code_default_size,
            bytes: &[0xb8, 0x34, 0x12, 0xbb, 0x78, 0x56], // MOV AX,1234; MOV BX,5678
            instruction_limit: 2,
        },
        BlockSnapshot {
            address: CodeAddress::CsRelative(0x1003),
            code_default_size,
            bytes: &[0xbb, 0x78, 0x56],
            instruction_limit: 1,
        },
        BlockSnapshot {
            address: CodeAddress::CsRelative(0x2000),
            code_default_size,
            bytes: &[0x0f, 0x44, 0xc0],
            instruction_limit: 1,
        },
        BlockSnapshot {
            address: CodeAddress::CsRelative(0xffff),
            code_default_size,
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
            eip: snapshot.address.eip(),
            instruction_count: 17,
            ..CpuState::real_mode()
        };
        image.map(snapshot.address.eip() >> 12, 0x3000, false);
        image.map((snapshot.address.eip() >> 12) + 1, 0x4000, false);
        image.data(0x3000 + (snapshot.address.eip() & 0xfff), snapshot.bytes);
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
        let code_default_size = SegmentDefaultSize::for_profile(profile);
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
                    address: CodeAddress::CsRelative(0x1000),
                    code_default_size,
                    bytes: code,
                    instruction_limit: 2,
                },
                BlockSnapshot {
                    address: CodeAddress::CsRelative(0x1100),
                    code_default_size,
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
        let mut module = TestModule::from_bytes(
            &blocks.bytes,
            &blocks.entries[0].export,
            Some(blocks.execution_target),
        );
        for entry in &blocks.entries {
            module.entry.clone_from(&entry.export);
            let mut image = Image::empty();
            image.cpu.instruction_count = 40;
            image.cpu.eip = entry.address.eip();
            let physical = profile == ExecutionProfile::Real16;
            // Physical JIT operands fit one page; virtual accesses also exercise
            // the shared helpers for scattered backing across a page boundary.
            image.cpu.registers.ecx = if physical { 0x2ff0 } else { 0x2fff };
            image.cpu.registers.ebx = if physical { 0x4ff0 } else { 0x4fff };
            if profile == ExecutionProfile::Real16 {
                image.initialize_real_mode();
            }
            image.map(1, 0x1000, false);
            image.data(entry.address.eip(), code);
            for (page, frame) in [(2, 0x4000), (3, 0x6000), (4, 0x8000), (5, 0xa000)] {
                image.map(page, frame, true);
            }
            image.data(0x4fff, &[0x11]);
            image.data(0x6000, &[0x22, 0x33, 0x44]);
            if physical {
                image.data(0x4ff0, &[0x11, 0x22, 0x33, 0x44]);
            }
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
                        ram: if physical {
                            &[(0x8ff0, &[0x11, 0x22, 0x33, 0x44])]
                        } else {
                            &[(0x8fff, &[0x11]), (0xa000, &[0x22, 0x33, 0x44])]
                        },
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
