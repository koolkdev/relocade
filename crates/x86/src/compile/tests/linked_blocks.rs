use crate::support::config::REAL16;
use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{CallPatches, TestModule},
    },
    CodeAddress::{self, CsRelative, RealMode},
    CompiledBlocks, Compiler, CpuState, DecodedBlock, ExecutionContext, ExecutionExit, Segment,
    SegmentDefaultSize, SegmentRegister, SnapshotInstruction,
};

mod input;

fn image(start: u32, code: &[u8]) -> Image {
    let mut image = Image::empty();
    image.cpu = CpuState {
        eip: start,
        ..CpuState::real_mode()
    };
    image.map(start >> 12, 0x3000, false);
    image.map((start >> 12) + 1, 0x4000, false);
    image.data(0x3000 + (start & 0xfff), code);
    image
}

fn block(address: CodeAddress, exported: bool, mut bytes: &[u8]) -> DecodedBlock {
    let mut eip = address.eip();
    let mut instructions = std::iter::from_fn(|| {
        if bytes.is_empty() {
            return None;
        }
        let instruction =
            SnapshotInstruction::decode(bytes, eip, SegmentDefaultSize::Bits16).unwrap();
        let next = instruction.next_eip();
        bytes = &bytes[next.wrapping_sub(eip) as usize..];
        eip = next;
        Some(instruction)
    });
    let mut block = DecodedBlock::new(address, instructions.next().unwrap(), exported).unwrap();
    for instruction in instructions {
        block.push(instruction).unwrap();
    }
    block
}

fn compile(blocks: Vec<DecodedBlock>) -> CompiledBlocks {
    Compiler::from_config(REAL16)
        .compile_decoded_blocks(blocks)
        .unwrap()
}

fn enter(blocks: &CompiledBlocks, address: CodeAddress) -> TestModule {
    TestModule::from_bytes(
        &blocks.bytes,
        &format!("block_{address}"),
        Some(blocks.config),
    )
}

#[test]
fn branches_link_to_supplied_blocks_and_dispatch_missing_destinations() {
    let code = [
        0xb8, 1, 0, // MOV AX,1
        0xe3, 5, // JCXZ 100A
        0xbb, 42, 0, // MOV BX,42
        0xeb, 0x76, // JMP 1080
        0xf1, // Unsupported destination
    ];
    let blocks = compile(vec![
        block(CsRelative(0x1000), true, &code[..5]),
        block(CsRelative(0x1005), false, &code[5..10]),
    ]);
    let module = enter(&blocks, CsRelative(0x1000));
    for count in [0, 1] {
        let mut image = image(0x1000, &code);
        image.cpu.registers.ecx = count;
        let mut after = image.cpu;
        after.registers.eax = 1;
        if count == 0 {
            after.eip = 0x100a;
            after.instruction_count = 2;
        } else {
            after.registers.ebx = 42;
            after.eip = 0x1080;
            after.instruction_count = 4;
        }
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[],
                    exit: Exit::Dispatch(after.eip),
                }],
            ),
        );
    }
}

#[test]
fn local_branches_and_linear_splits_retain_every_checkpoint() {
    let code = [
        0xb9, 3, 0, // MOV CX,3
        0xbb, 42, 0, // MOV BX,42
        0xe2, 0xfb, // LOOP 1003
        0xb8, 7, 0, // MOV AX,7
        0xeb, 0x73, // JMP 1080, outside this batch
    ];
    let blocks = compile(vec![
        block(CsRelative(0x1000), true, &code[..3]),
        block(CsRelative(0x1003), false, &code[3..6]),
        block(CsRelative(0x1006), false, &code[6..8]),
        block(CsRelative(0x1008), false, &code[8..]),
    ]);
    let module = enter(&blocks, CsRelative(0x1000));
    let image = image(0x1000, &code);
    for (interrupt, budget) in [(false, 0u64), (true, 0), (false, 4)] {
        let mut input = image.input();
        input.patches_before_calls.push(CallPatches::cpu(vec![
            (
                std::mem::offset_of!(ExecutionContext, interrupt_requested) as u32,
                vec![u8::from(interrupt)],
            ),
            (
                std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
                budget.to_le_bytes().to_vec(),
            ),
        ]));
        let mut after = image.cpu;
        let exit = if interrupt {
            after.registers.ecx = 3;
            after.eip = 0x1003;
            after.instruction_count = 1;
            Exit::Other(ExecutionExit::Interrupted)
        } else if budget != 0 {
            after.registers.ecx = 2;
            after.registers.ebx = 42;
            after.eip = 0x1006;
            after.instruction_count = 4;
            Exit::Other(ExecutionExit::BudgetExhausted)
        } else {
            after.registers.ecx = 0;
            after.registers.ebx = 42;
            after.registers.eax = 7;
            after.eip = 0x1080;
            after.instruction_count = 9;
            Exit::Dispatch(0x1080)
        };
        assert_eq!(
            module.observe(&input, 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[],
                    exit
                }]
            ),
        );
    }
}

#[test]
fn a_linked_successor_keeps_its_fetch_guard() {
    let code = [0xb8, 1, 0, 0xbb, 2, 0, 0xba, 3, 0, 0xeb, 0];
    let blocks = compile(vec![
        block(CsRelative(0xfffa), true, &code[..3]),
        block(CsRelative(0xfffd), false, &code[3..6]),
        block(CsRelative(0x10000), false, &code[6..]),
    ]);
    let module = enter(&blocks, CsRelative(0xfffa));
    let image = image(0xfffa, &code);
    let mut after = image.cpu;
    after.registers.eax = 1;
    after.registers.ebx = 2;
    after.eip = 0x10000;
    after.instruction_count = 2;
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[],
                exit: Exit::Interpret
            }]
        ),
    );
}

#[test]
fn equal_offsets_in_different_segments_link_to_their_own_code() {
    let codes = [
        [0xb8, 1, 0, 0xbb, 2, 0, 0xeb, 0],
        [0xb8, 3, 0, 0xbb, 4, 0, 0xeb, 0],
    ];
    let roots = [0, 0x10].map(|segment| CodeAddress::RealMode {
        segment,
        eip: 0x1000,
    });
    let mut plan = Vec::new();
    for (root, code) in roots.into_iter().zip(&codes) {
        plan.push(block(root, true, &code[..3]));
        plan.push(block(root.with_eip(0x1003), false, &code[3..]));
    }
    let blocks = compile(plan);
    for (index, (root, code)) in roots.into_iter().zip(&codes).enumerate() {
        let module = enter(&blocks, root);
        let mut image = image(0x1000, code);
        image.cpu.segments[Segment::Cs] =
            SegmentRegister::real_mode(Segment::Cs, index as u16 * 0x10);
        image.data(0x3000 + index as u32 * 0x100, code);
        let mut after = image.cpu;
        after.registers.eax = 1 + index as u32 * 2;
        after.registers.ebx = 2 + index as u32 * 2;
        after.eip = 0x1008;
        after.instruction_count = 3;
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[],
                    exit: Exit::Dispatch(0x1008)
                }]
            ),
        );
    }
}

#[test]
fn a_far_jump_cannot_use_the_callers_local_offset() {
    let code = [
        0xe3, 7, // JCXZ 1009
        0xea, 9, 0x10, 0x20, 0, // JMP FAR 0020:1009
        0x90, 0x90, 0xbb, 42, 0, // MOV BX,42
        0xeb, 0,
    ];
    let blocks = compile(vec![
        block(CsRelative(0x1000), true, &code[..2]),
        block(CsRelative(0x1002), false, &code[2..7]),
        block(CsRelative(0x1009), false, &code[9..]),
    ]);
    let module = enter(&blocks, CsRelative(0x1000));
    for count in [0, 1] {
        let mut image = image(0x1000, &code);
        image.cpu.registers.ecx = count;
        let mut after = image.cpu;
        let destination = if count == 0 {
            after.registers.ebx = 42;
            after.instruction_count = 3;
            0x100e
        } else {
            after.segments[Segment::Cs] = SegmentRegister::real_mode(Segment::Cs, 0x20);
            after.instruction_count = 2;
            0x1009
        };
        after.eip = destination;
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[],
                    exit: Exit::Dispatch(destination)
                }]
            ),
        );
    }
}

#[test]
fn a_near_call_uses_the_resolved_entry_and_ret_dispatches_its_saved_ip() {
    let code = [
        0xe8, 5, 0, // CALL 1008
        0x74, 7, // JZ 100C
        0xeb, 1, // JMP 1008
        0x90, 0xb8, 42, 0, 0xc3, // 1008: MOV AX,42; RET
        0xb8, 73, 0, 0xc3, // 100C: MOV AX,73; RET
    ];
    let blocks = compile(vec![
        block(CsRelative(0x1000), true, &code[..3]),
        block(CsRelative(0x1003), true, &code[3..5]),
        block(CsRelative(0x1005), false, &code[5..7]),
        block(CsRelative(0x1008), false, &code[8..12]),
        block(CsRelative(0x100c), true, &code[12..]),
    ]);
    let module = enter(&blocks, CsRelative(0x1000));
    let mut image = image(0x1000, &code);
    image.cpu.registers.esp = 0x800;
    image.map(0, 0, true);
    let mut input = image.input();
    input.call_target = Some((0x1008, "block_100c".into()));
    let mut after = image.cpu;
    after.registers.eax = 73;
    after.eip = 0x1003;
    after.instruction_count = 3;
    assert_eq!(
        module.observe(&input, 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[(0x7fe, &[3, 0x10])],
                exit: Exit::Dispatch(0x1003),
            }]
        ),
    );
}

#[test]
fn known_calls_share_slots_by_full_address_even_for_supplied_callees() {
    let code = [
        0xe8, 0xfd, 0, // CALL 0200
        0xe8, 0xfa, 0, // CALL 0200
        0x9a, 0, 2, 0x10, 0, // CALL FAR 0010:0200
        0xff, 0xd0, // CALL AX
    ];
    let calls = |start: CodeAddress| {
        vec![
            block(start, true, &code[..3]),
            block(start.with_eip(0x103), true, &code[3..6]),
            block(start.with_eip(0x106), true, &code[6..11]),
            block(start.with_eip(0x10b), true, &code[11..]),
            block(start.with_eip(0x200), false, &[0xc3]),
        ]
    };
    let mut plan = calls(RealMode {
        segment: 0x10,
        eip: 0x100,
    });
    plan.extend(calls(RealMode {
        segment: 0x20,
        eip: 0x100,
    }));
    let blocks = compile(plan);
    assert_eq!(blocks.call_targets, [0x0010_0200, 0x0020_0200]);

    // Only the far CALL supplies its own segment when CS is unknown.
    let blocks = compile(calls(CsRelative(0x100)));
    assert_eq!(blocks.call_targets, [0x0010_0200]);
}

#[test]
fn two_public_jumps_share_a_private_block_and_its_source_span() {
    let start = RealMode {
        segment: 0x10,
        eip: 0x100,
    };
    let second = start.with_eip(0x110);
    let tail = start.with_eip(0x120);
    let blocks = compile(vec![
        block(start, true, &[0xeb, 0x1e]),             // JMP 0120
        block(second, true, &[0xeb, 0x0e]),            // JMP 0120
        block(tail, false, &[0xba, 7, 0, 0xff, 0xe3]), // MOV DX,7; JMP BX
    ]);

    assert_eq!(blocks.spans, [(start, 2), (second, 2), (tail, 5)]);
    let wasm = relocade_test_support::Module::new(&blocks.bytes);
    let expected = ["block_0010:0100", "block_0010:0110"];
    assert_eq!(
        blocks
            .entries
            .iter()
            .map(|entry| entry.export.as_str())
            .collect::<Vec<_>>(),
        expected,
    );
    for export in expected {
        assert!(wasm.wasmtime().get_export(export).is_some());
    }
    assert!(wasm.wasmtime().get_export("block_0010:0120").is_none());
}

#[test]
fn a_jump_cannot_link_to_another_decode_width() {
    let target = CsRelative(0x1010);
    let other_width =
        SnapshotInstruction::decode(&[0x90], target.eip(), SegmentDefaultSize::Bits32).unwrap();
    let blocks = compile(vec![
        block(CsRelative(0x1000), true, &[0xeb, 0x0e]),
        DecodedBlock::new(target, other_width, false).unwrap(),
    ]);
    let module = enter(&blocks, CsRelative(0x1000));
    let image = image(0x1000, &[0xeb, 0x0e]);
    let mut after = image.cpu;
    after.eip = target.eip();
    after.instruction_count = 1;
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[],
                exit: Exit::Dispatch(target.eip())
            }]
        ),
    );
}
