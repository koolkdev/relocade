use super::*;
use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{CallPatches, TestModule},
    },
    CpuState, ExecutionContext, ExecutionExit, Segment, SegmentRegister,
};

fn limits() -> FunctionLimits {
    FunctionLimits {
        block_instructions: 1,
        ..FunctionLimits::default()
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
        .compile_function(start, limits(), reader(start, code))
        .unwrap();
    TestModule::from_bytes(
        &blocks.bytes,
        &blocks.entries[0].export,
        Some(blocks.execution_target),
    )
}

#[test]
fn local_branches_and_linear_splits_retain_every_checkpoint() {
    let code = [
        0xb9, 3, 0, // MOV CX,3
        0xbb, 42, 0, // MOV BX,42
        0xe2, 0xfb, // LOOP 1003
        0xb8, 7, 0, // MOV AX,7
        0xeb, 0x73, // JMP 1080, outside the discovered function
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
fn function_namespaces_do_not_alias_equal_offsets() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let mut module = compiler.function_module();
    let codes = [
        [0xb8, 1, 0, 0xbb, 2, 0, 0xeb, 0],
        [0xb8, 3, 0, 0xbb, 4, 0, 0xeb, 0],
    ];
    let mut roots = Vec::new();
    for code in &codes {
        roots.push(
            module
                .add_function(0x1000, None, limits(), reader(0x1000, code))
                .unwrap()[0]
                .export
                .clone(),
        );
    }
    let blocks = module.compile().unwrap();
    for (index, (root, code)) in roots.iter().zip(&codes).enumerate() {
        let module = TestModule::from_bytes(&blocks.bytes, root, Some(blocks.execution_target));
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
fn a_near_call_to_a_local_entry_keeps_the_guest_return_frame() {
    let code = [
        0xe8, 5, 0, // CALL 1008
        0xeb, 3, // JMP 1008 discovers the callee as a local block too
        0x90, 0x90, 0x90, 0xb8, 42, 0, 0xc3, // MOV AX,42; RET
    ];
    let module = compile(0x1000, &code);
    let mut image = image(0x1000, &code);
    image.cpu.registers.esp = 0x800;
    image.map(0, 0, true);
    let mut after = image.cpu;
    after.registers.eax = 42;
    after.eip = 0x1003;
    after.instruction_count = 3;
    assert_eq!(
        module.observe(&image.input(), 1),
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
fn call_table_slots_are_shared_by_exact_targets_across_functions() {
    let compiler = Compiler::new(ExecutionProfile::Real16);
    let mut module = compiler.function_module();
    let code = [
        0xe8, 0xfd, 0, // CALL 0200
        0xe8, 0xfa, 0, // CALL 0200 again
        0x9a, 0, 2, 0x10, 0, // CALL FAR 0010:0200
        0xff, 0xd0, // CALL AX has no constant destination
        0xc3,
    ];
    for (cs, start, bytes) in [
        (0x10, 0x100, code.as_slice()),
        (0x10, 0x180, &[0xe8, 0x7d, 0, 0xc3][..]), // CALL 0200; RET
        (0x20, 0x100, code.as_slice()),
    ] {
        module
            .add_function(
                start,
                Some(cs),
                FunctionLimits::default(),
                reader(start, bytes),
            )
            .unwrap();
    }
    let blocks = module.compile().unwrap();
    assert_eq!(blocks.call_targets, [0x0010_0200, 0x0020_0200]);

    // Without a known CS, only the explicit far target has a table entry.
    let unknown_cs = compiler
        .compile_function(0x100, FunctionLimits::default(), reader(0x100, &code))
        .unwrap();
    assert_eq!(unknown_cs.call_targets, [0x0010_0200]);

    let mut local = compiler.function_module();
    local
        .add_function(
            0x100,
            Some(0x10),
            FunctionLimits::default(),
            reader(0x100, &[0xe8, 0, 0, 0xc3]),
        )
        .unwrap();
    assert!(
        local.compile().unwrap().call_targets.is_empty(),
        "a local call uses its direct entry"
    );
}
