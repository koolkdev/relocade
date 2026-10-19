use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{CallPatches, TestModule},
    },
    BlockSnapshot, Compiler, CpuState, ExecutionContext, ExecutionExit, SegmentProfile,
};
use wasmparser::{Operator, Parser, Payload, TypeRef};

fn local_call_count(bytes: &[u8]) -> usize {
    let mut imports = 0;
    let mut calls = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(section) => {
                for import in section {
                    if matches!(import.unwrap().ty, TypeRef::Func(_)) {
                        imports += 1;
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                for op in body.get_operators_reader().unwrap() {
                    if matches!(op.unwrap(), Operator::ReturnCall { function_index } if function_index >= imports)
                    {
                        calls += 1;
                    }
                }
            }
            _ => {}
        }
    }
    calls
}

fn cycles_and_conditional_exits() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    let snapshots = [
        BlockSnapshot {
            start_eip: 0x1000,
            bytes: &[0xb8, 1, 0, 0, 0, 0xeb, 9],
            instruction_limit: 2,
        },
        BlockSnapshot {
            start_eip: 0x1010,
            bytes: &[0xe2, 0xee],
            instruction_limit: 1,
        },
        BlockSnapshot {
            start_eip: 0x1012,
            bytes: &[0xbb, 0x78, 0x56, 0, 0],
            instruction_limit: 1,
        },
    ];
    for local_fallthrough in [false, true] {
        let blocks = compiler
            .compile_blocks(&snapshots[..if local_fallthrough { 3 } else { 2 }])
            .unwrap();
        assert_eq!(
            local_call_count(&blocks.bytes),
            if local_fallthrough { 3 } else { 2 }
        );
        let module =
            TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
        let mut image = Image::empty();
        image.cpu = CpuState {
            eip: 0x1000,
            ..CpuState::default()
        };
        image.cpu.registers.ecx = 3;
        for target in [2_u64, 3, 9, 10, 100] {
            let mut input = image.input();
            input.patches_before_calls.push(CallPatches::cpu(vec![(
                std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
                target.to_le_bytes().to_vec(),
            )]));
            let mut after = image.cpu;
            after.registers.eax = 1;
            let (count, eip, ecx) = match target {
                2 => (2, 0x1010, 3),
                3 => (3, 0x1000, 2),
                9 => (9, 0x1012, 0),
                _ if !local_fallthrough => (9, 0x1012, 0),
                _ => {
                    after.registers.ebx = 0x5678;
                    (10, 0x1017, 0)
                }
            };
            after.instruction_count = count;
            after.eip = eip;
            after.registers.ecx = ecx;
            let exit = if count >= target {
                Exit::Other(ExecutionExit::BudgetExhausted.to_bits())
            } else {
                Exit::Dispatch(eip)
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
                )
            );
        }
    }
}

#[test]
fn known_successors_link_but_context_changes_and_indirect_jumps_dispatch() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    for code in [
        &[0xff, 0xe0][..],            // JMP EAX
        &[0x8e, 0xd8],                // MOV DS,AX
        &[0xe6, 0x80],                // OUT 80,AL
        &[0x0f, 0xa2],                // CPUID
        &[0xea, 0, 0x20, 0, 0, 0, 0], // far JMP to a supplied EIP
    ] {
        let blocks = compiler
            .compile_blocks(&[
                BlockSnapshot {
                    start_eip: 0x1000,
                    bytes: code,
                    instruction_limit: 1,
                },
                BlockSnapshot {
                    start_eip: 0x1000 + code.len() as u32,
                    bytes: &[0x90],
                    instruction_limit: 1,
                },
                BlockSnapshot {
                    start_eip: 0x2000,
                    bytes: &[0x90],
                    instruction_limit: 1,
                },
            ])
            .unwrap();
        assert_eq!(local_call_count(&blocks.bytes), 0, "{code:x?}");
    }
    let blocks = compiler
        .compile_blocks(&[BlockSnapshot {
            start_eip: 0x1000,
            bytes: &[0xeb, 0xfe],
            instruction_limit: 1,
        }])
        .unwrap();
    assert_eq!(local_call_count(&blocks.bytes), 1);
    assert_eq!(
        local_call_count(
            &compiler
                .compile_block(0x1000, &[0xeb, 0xfe], 1)
                .unwrap()
                .bytes
        ),
        0
    );
}

#[test]
fn linked_cycles_keep_budget_boundaries() {
    cycles_and_conditional_exits();
}
