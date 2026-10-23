mod transfers;

use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{CallPatches, TestModule},
    },
    BlockSnapshot, Compiler, CpuState, ExecutionContext, ExecutionExit, SegmentProfile,
};
use wasmparser::{Operator, Parser, Payload, TypeRef};

fn entry_operators<'a>(bytes: &'a [u8], export: &str) -> Vec<Operator<'a>> {
    let mut function = 0;
    let mut entry = None;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(imports) => {
                function += imports
                    .into_iter()
                    .filter(|import| matches!(import.as_ref().unwrap().ty, TypeRef::Func(_)))
                    .count() as u32;
            }
            Payload::ExportSection(exports) => {
                entry = exports
                    .into_iter()
                    .map(Result::unwrap)
                    .find(|item| item.name == export)
                    .map(|item| item.index);
            }
            Payload::CodeSectionEntry(body) => {
                if Some(function) == entry {
                    return body
                        .get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .map(Result::unwrap)
                        .collect();
                }
                function += 1;
            }
            _ => {}
        }
    }
    panic!("missing export {export}");
}

#[test]
fn conditional_links_use_the_original_condition_for_each_arm() {
    for (local_taken, local_fallthrough) in
        [(true, true), (true, false), (false, true), (false, false)]
    {
        let mut snapshots = vec![BlockSnapshot {
            start_eip: 0x1000,
            bytes: &[0xe3, 0x0e],
            instruction_limit: 1, // JECXZ 1010
        }];
        if local_taken {
            snapshots.push(BlockSnapshot {
                start_eip: 0x1010,
                bytes: &[0xbb, 1, 0, 0, 0],
                instruction_limit: 1,
            });
        }
        if local_fallthrough {
            snapshots.push(BlockSnapshot {
                start_eip: 0x1002,
                bytes: &[0xbb, 2, 0, 0, 0],
                instruction_limit: 1,
            });
        }
        let blocks = Compiler::new(SegmentProfile::Flat32)
            .compile_blocks(&snapshots)
            .unwrap();
        let operators = entry_operators(&blocks.bytes, "block_1000");
        // JECXZ tests zero directly. Linking must not compare the selected EIP
        // with destination addresses or lower an address switch to a table.
        assert!(operators.iter().any(|op| matches!(op, Operator::I32Eqz)));
        assert!(!operators.iter().any(|op| matches!(
            op,
            Operator::I32Eq | Operator::I32Ne | Operator::BrTable { .. }
        )));
        assert_eq!(
            operators
                .iter()
                .filter(|op| matches!(op, Operator::ReturnCall { .. }))
                .count(),
            if local_taken || local_fallthrough {
                2
            } else {
                1
            }
        );
        assert_eq!(
            local_call_count(&blocks.bytes),
            usize::from(local_taken) + usize::from(local_fallthrough)
        );
        let module =
            TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
        for count in [0, 1] {
            let mut image = Image::empty();
            image.cpu = CpuState {
                eip: 0x1000,
                ..CpuState::default()
            };
            image.cpu.registers.ecx = count;
            let mut after = image.cpu;
            after.eip = if count == 0 { 0x1010 } else { 0x1002 };
            after.instruction_count = 1;
            let linked = if count == 0 {
                local_taken
            } else {
                local_fallthrough
            };
            if linked {
                after.registers.ebx = if count == 0 { 1 } else { 2 };
                after.eip += 5;
                after.instruction_count += 1;
            }
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: after,
                        ram: &[],
                        exit: Exit::Dispatch(after.eip),
                    }]
                )
            );
        }
    }
}

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

#[test]
fn linked_cycles_keep_budget_boundaries() {
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
                Exit::Other(ExecutionExit::BudgetExhausted)
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
fn context_changes_and_unresolved_transfers_dispatch() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
    for code in [
        &[0xff, 0xe0][..],               // JMP EAX
        &[0xff, 0xd0],                   // CALL EAX
        &[0xc3],                         // RET
        &[0x8e, 0xd8],                   // MOV DS,AX
        &[0x0f, 0xa2],                   // CPUID
        &[0xea, 0, 0x20, 0, 0, 0x1b, 0], // far JMP to a supplied EIP
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
}

#[test]
fn single_block_compilation_keeps_dispatch_for_self_loops() {
    let compiler = Compiler::new(SegmentProfile::Flat32);
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
