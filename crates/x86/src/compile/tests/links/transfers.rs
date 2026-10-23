use super::*;
use crate::{support::step::Event, ExecutionProfile, Segments};

#[test]
fn a_port_callback_returns_through_dispatch_before_the_local_successor() {
    let blocks = Compiler::new(ExecutionProfile::Real16)
        .compile_blocks(&[
            BlockSnapshot {
                start_eip: 0x1000,
                bytes: &[0xe6, 0x80], // OUT 80,AL
                instruction_limit: 1,
            },
            BlockSnapshot {
                start_eip: 0x1002,
                bytes: &[0xbb, 1, 0], // MOV BX,1
                instruction_limit: 1,
            },
        ])
        .unwrap();
    assert_eq!(local_call_count(&blocks.bytes), 0);
    let module = TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
    let mut image = Image::empty();
    image.cpu = CpuState {
        eip: 0x1000,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    image.cpu.registers.eax = 0x12;
    let mut after = image.cpu;
    after.eip = 0x1002;
    after.instruction_count = 1;
    let mut expected = expected(
        &image,
        &[Step {
            cpu: after,
            ram: &[],
            exit: Exit::Dispatch(0x1002),
        }],
    );
    expected.events.insert(
        0,
        Event::PortWrite {
            port: 0x80,
            bytes: 1,
            value: 0x12,
        },
    );
    assert_eq!(module.observe(&image.input(), 1), expected);
}

#[test]
fn a_branch_keeps_its_condition_when_pending_flags_are_published() {
    let blocks = Compiler::new(SegmentProfile::Flat32)
        .compile_blocks(&[
            BlockSnapshot {
                start_eip: 0x1000,
                bytes: &[0xf5, 0x72, 0x0d], // CMC; JC 1010
                instruction_limit: 2,
            },
            BlockSnapshot {
                start_eip: 0x1010,
                bytes: &[0xbb, 1, 0, 0, 0],
                instruction_limit: 1,
            },
            BlockSnapshot {
                start_eip: 0x1003,
                bytes: &[0xbb, 2, 0, 0, 0],
                instruction_limit: 1,
            },
        ])
        .unwrap();
    let module = TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
    for carry in [0, 1] {
        let mut image = Image::empty();
        image.cpu = CpuState {
            eip: 0x1000,
            ..CpuState::default()
        };
        image.cpu.flags.bytes.cf = carry;
        let mut after = image.cpu;
        after.flags.bytes.cf = 1 - carry;
        after.registers.ebx = if carry == 0 { 1 } else { 2 };
        after.eip = if carry == 0 { 0x1015 } else { 0x1008 };
        after.instruction_count = 3;
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

#[test]
fn linked_call_pushes_its_return_address_and_ret_uses_dispatch() {
    let blocks = Compiler::new(SegmentProfile::Flat32)
        .compile_blocks(&[
            BlockSnapshot {
                start_eip: 0x1000,
                bytes: &[0xe8, 0xfb, 0x0f, 0, 0], // CALL 2000
                instruction_limit: 1,
            },
            BlockSnapshot {
                start_eip: 0x1005,
                bytes: &[0xb9, 0xff, 0xff, 0xff, 0xff], // must not execute after RET
                instruction_limit: 1,
            },
            BlockSnapshot {
                start_eip: 0x2000,
                bytes: &[0xbb, 0x78, 0x56, 0x34, 0x12, 0xc3], // MOV EBX,12345678; RET
                instruction_limit: 2,
            },
        ])
        .unwrap();
    let module = TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
    let mut image = Image::empty();
    image.cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    image.cpu.registers.esp = 0x7000;
    image.map(6, 0x6000, true);
    for target in [1_u64, u64::MAX] {
        let mut input = image.input();
        input.patches_before_calls.push(CallPatches::cpu(vec![(
            std::mem::offset_of!(ExecutionContext, instruction_target) as u32,
            target.to_le_bytes().to_vec(),
        )]));
        let mut after = image.cpu;
        let exit = if target == 1 {
            after.registers.esp = 0x6ffc;
            after.eip = 0x2000;
            after.instruction_count = 1;
            Exit::Other(ExecutionExit::BudgetExhausted)
        } else {
            after.registers.ebx = 0x1234_5678;
            after.eip = 0x1005;
            after.instruction_count = 3;
            Exit::Dispatch(0x1005)
        };
        assert_eq!(
            module.observe(&input, 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[(0x6ffc, &[0x05, 0x10, 0, 0])],
                    exit,
                }]
            )
        );
    }
}

#[test]
fn a_link_enters_the_destination_fetch_guard_before_its_effects() {
    let blocks = Compiler::new(SegmentProfile::Segmented32)
        .compile_blocks(&[
            BlockSnapshot {
                start_eip: 0x1000,
                bytes: &[0xeb, 0x0e], // JMP 1010; target itself is within CS
                instruction_limit: 1,
            },
            BlockSnapshot {
                start_eip: 0x1010,
                bytes: &[0xb8, 1, 0, 0, 0], // encoding extends beyond CS
                instruction_limit: 1,
            },
        ])
        .unwrap();
    let module = TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
    let mut image = Image::empty();
    image.cpu = CpuState {
        eip: 0x1000,
        ..CpuState::default()
    };
    image.cpu.segments.cs.limit = 0x1010;
    let mut after = image.cpu;
    after.eip = 0x1010;
    after.instruction_count = 1;
    assert_eq!(
        module.observe(&image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[],
                exit: Exit::Interpret,
            }]
        )
    );
}

#[test]
fn a_denied_taken_target_faults_before_loop_retirement() {
    let blocks = Compiler::new(SegmentProfile::Segmented32)
        .compile_blocks(&[
            BlockSnapshot {
                start_eip: 0x1000,
                bytes: &[0xe2, 0x0e], // LOOP 1010
                instruction_limit: 1,
            },
            BlockSnapshot {
                start_eip: 0x1010,
                bytes: &[0x90],
                instruction_limit: 1,
            },
        ])
        .unwrap();
    let module = TestModule::from_bytes(&blocks.bytes, "block_1000", Some(blocks.execution_target));
    for count in [1, 2] {
        let mut image = Image::empty();
        image.cpu = CpuState {
            eip: 0x1000,
            ..CpuState::default()
        };
        image.cpu.registers.ecx = count;
        image.cpu.segments.cs.limit = 0x100f;
        let mut after = image.cpu;
        let exit = if count == 1 {
            after.registers.ecx = 0;
            after.eip = 0x1002;
            after.instruction_count = 1;
            Exit::Dispatch(0x1002)
        } else {
            Exit::GeneralProtection { error: 0 }
        };
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[],
                    exit,
                }]
            )
        );
    }
}
