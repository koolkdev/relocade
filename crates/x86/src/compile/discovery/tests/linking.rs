use super::*;
use crate::{
    real_mode::FarPointer,
    support::{
        machine::{expected, Exit, Image, Step},
        step::{CallPatches, TestModule},
    },
    CpuState, ExecutionContext, ExecutionExit, Segment, SegmentRegister,
};

fn limits() -> DiscoveryLimits {
    DiscoveryLimits {
        block_instructions: 1,
        ..DiscoveryLimits::default()
    }
}

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

fn compile(start: u32, code: &[u8]) -> TestModule {
    let blocks = Compiler::new(ExecutionProfile::Real16)
        .compile_blocks_from_root(start, limits(), reader(start, code))
        .unwrap();
    TestModule::from_bytes(
        &blocks.bytes,
        &blocks.entries[0].export,
        Some(blocks.execution_target),
    )
}

#[test]
fn links_keep_valid_paths_and_dispatch_undecodable_destinations() {
    let code = [
        0xb8, 1, 0, // MOV AX,1
        0xe3, 5, // JCXZ 100A
        0xbb, 42, 0, // MOV BX,42
        0xeb, 0x76, // JMP 1080
        0xf1, // Unsupported destination
    ];
    let module = compile(0x1000, &code);
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
        0xeb, 0x73, // JMP 1080, outside the discovered region
    ];
    let module = compile(0x1000, &code);
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
    let module = compile(0xfffa, &code);
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
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let codes = [
        [0xb8, 1, 0, 0xbb, 2, 0, 0xeb, 0],
        [0xb8, 3, 0, 0xbb, 4, 0, 0xeb, 0],
    ];
    let roots = [0, 0x10].map(|segment| CodeAddress::RealMode {
        segment,
        eip: 0x1000,
    });
    let mut module =
        compiler.discover_blocks(mapped_code([(roots[0], &codes[0]), (roots[1], &codes[1])]));
    for root in roots {
        module
            .add_root(root, SegmentDefaultSize::Bits16, limits())
            .unwrap();
    }
    let blocks = module.compile().unwrap();
    for (index, (root, code)) in roots.iter().zip(&codes).enumerate() {
        let module = TestModule::from_bytes(
            &blocks.bytes,
            &format!("block_{root}"),
            Some(blocks.execution_target),
        );
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
    let module = compile(0x1000, &code);
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
        0x74, 7, // JZ 100C discovers the replacement
        0xeb, 1, // JMP 1008 discovers the original callee
        0x90, 0xb8, 42, 0, 0xc3, // 1008: MOV AX,42; RET
        0xb8, 73, 0, 0xc3, // 100C: MOV AX,73; RET
    ];
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let start = CodeAddress::CsRelative(0x1000);
    let mut discovery = compiler.discover_blocks(mapped_code([(start, code.as_slice())]));
    for root in [start, CodeAddress::CsRelative(0x100c)] {
        discovery
            .add_root(root, SegmentDefaultSize::Bits16, limits())
            .unwrap();
    }
    let blocks = discovery.compile().unwrap();
    assert_eq!(exported_eips(&blocks), [0x1000, 0x1003, 0x100c]);
    let module = TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
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
fn call_table_slots_are_shared_by_exact_targets_across_roots() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let code = [
        0xe8, 0xfd, 0, // CALL 0200
        0xe8, 0xfa, 0, // CALL 0200 again
        0x9a, 0, 2, 0x10, 0, // CALL FAR 0010:0200
        0xff, 0xd0, // CALL AX has no constant destination
        0xc3,
    ];
    let ranges = [
        (0x10, 0x100, code.as_slice()),
        (0x10, 0x180, &[0xe8, 0x7d, 0, 0xc3][..]), // CALL 0200; RET
        (0x20, 0x100, code.as_slice()),
    ]
    .map(|(segment, offset, bytes)| (CodeAddress::from(FarPointer { segment, offset }), bytes));
    let mut module = compiler.discover_blocks(mapped_code(ranges));
    for (entry, _) in ranges {
        module
            .add_root(
                entry,
                SegmentDefaultSize::Bits16,
                DiscoveryLimits::default(),
            )
            .unwrap();
    }
    let blocks = module.compile().unwrap();
    assert_eq!(blocks.call_targets, [0x0010_0200, 0x0020_0200]);

    // Without a known CS, only the explicit far target has a table entry.
    let unknown_cs = compiler
        .compile_blocks_from_root(0x100, DiscoveryLimits::default(), reader(0x100, &code))
        .unwrap();
    assert_eq!(unknown_cs.call_targets, [0x0010_0200]);
}
