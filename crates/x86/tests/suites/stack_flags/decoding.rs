use super::{pop_case, push_case, stored_flags, IMAGES};
use crate::support::encoding::check_length;
use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    guest::{Exit, Machine},
    machine::{check, Image, Step},
    step::{Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32::Esp};

fn boundary_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let image = IMAGES[12];
    for opcode in [0x9c, 0x9d] {
        for prefixes in [0, 1, 2, 14] {
            let code = [vec![0x66; prefixes], vec![opcode]].concat();
            let width = if prefixes == 0 { 4 } else { 2 };
            for origin in [0x2000 - code.len() as u32, u32::MAX] {
                let name = format!("{opcode:02x}, {prefixes} operand prefixes, EIP {origin:08x}");
                let case = if opcode == 0x9c {
                    push_case(name, &code, image)
                        .register(Esp, 0x9004, 0x9004 - width)
                        .memory(0x9000, &[0xa5; 8], ReadWrite)
                        .expect_memory(0x9004 - width, &image.bits.to_le_bytes()[..width as usize])
                } else {
                    pop_case(name, &code, image, width == 2)
                        .register(Esp, 0x9000, 0x9000 + width)
                        .memory(0x9000, &image.bits.to_le_bytes(), ReadOnly)
                };
                cases.push(case.at(origin).instruction_count(u32::MAX));
            }
        }
    }
    cases
}

#[test]
fn complete_stack_flag_opcodes_need_no_operand_or_next_byte() {
    for opcode in [0x9c, 0x9d] {
        for prefixes in [0, 1, 2, 14] {
            let code = [vec![0x66; prefixes], vec![opcode]].concat();
            check_length(&code);
        }
    }
}

#[test]
fn unsupported_prefixes_precede_flag_and_stack_access() {
    for prefix in [0xf0, 0xf2, 0xf3] {
        for opcode in [0x9c, 0x9d] {
            for code in [vec![prefix, opcode], vec![0x66, prefix, opcode]] {
                assert_eq!(
                    compile_block_from_bytes(0x1000, &code, 1).err(),
                    Some(BlockError::UnsupportedInstruction {
                        address: 0x1000,
                        opcode: prefix
                    })
                );
                let image = Image::new(&code);
                check(
                    TestModule::interpreter(),
                    "unsupported prefix leaves the complete CPU and stack unchanged",
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: Exit::Other(0x0008_0000_0000_1000 | (u64::from(prefix) << 32)),
                    }],
                );
            }
        }
    }
}

#[test]
fn a_missing_opcode_or_exhausted_length_budget_precedes_stack_access() {
    for count in [1, 14, 15] {
        let start = 0x2000 - count;
        let mut image = Image::new(&[]);
        image.cpu.eip = start;
        image.cpu.registers.esp = 0x6000;
        image.data(0x4000 - count, &vec![0x66; count as usize]);
        let exit = if count == 15 {
            Exit::Other(0x0002_0000_0000_0000)
        } else {
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            }
        };
        check(
            TestModule::interpreter(),
            "the opcode is required before stack access",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            }],
        );
    }
    for opcode in [0x9c, 0x9d] {
        let code = [vec![0x66; 15], vec![opcode]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &code, 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 })
        );
    }
}

#[test]
fn a_completed_flag_stack_transfer_survives_the_following_fetch_fault() {
    let observer = TestModule::new(&crate::state::compile_flag_observer().unwrap());
    for opcode in [0x9c, 0x9d] {
        for prefixes in [0, 1, 14] {
            let code = [vec![0x66; prefixes], vec![opcode]].concat();
            let width = if prefixes == 0 { 4 } else { 2 };
            let mut machine = Machine::at(0x2000 - code.len() as u32, &code);
            machine.cpu.flags = stored_flags(63, 31);
            machine.cpu.registers.esp = if opcode == 0x9c { 0x9004 } else { 0x9000 };
            machine.memory(0x9000, &[0; 8], ReadWrite);
            let initial = machine.state();
            let executions = machine.run_many(TestModule::interpreter(), Engine::Wasmtime, 2);
            let completed = &executions[0];
            let fault = &executions[1];
            let mut expected = initial.clone();
            expected.cpu.eip = 0x2000;
            expected.cpu.instruction_count = 0;
            if opcode == 0x9c {
                expected.cpu.registers.esp -= width;
                expected.memory.write(
                    expected.cpu.registers.esp,
                    &IMAGES[12].bits.to_le_bytes()[..width as usize],
                );
            } else {
                expected.cpu.registers.esp += width;
                let bytes = completed.state.cpu.flags.bytes;
                let cpu = completed.state.cpu.to_bytes();
                assert_eq!(
                    observer.observe(&Input::new(&cpu), 1),
                    Observation {
                        events: vec![Event::Return {
                            outcome: Outcome::Returned(vec![Argument::I32(0); 6]),
                            snapshot: Snapshot {
                                cpu: cpu.to_vec(),
                                guest: None
                            },
                        }],
                        guest_unchanged: true,
                        machine_unchanged: true,
                    }
                );
                assert_eq!([bytes.tf, bytes.df, bytes.nt], [0; 3]);
                assert_eq!(
                    [bytes.ac, bytes.id],
                    if width == 2 {
                        [initial.cpu.flags.bytes.ac, initial.cpu.flags.bytes.id]
                    } else {
                        [0, 0]
                    }
                );
                assert_eq!(bytes.reserved, initial.cpu.flags.bytes.reserved);
                assert_eq!(
                    completed.state.cpu.flags.status_source.reserved,
                    initial.cpu.flags.status_source.reserved
                );
                // Logical status was checked above; its backing form is owned by state.
                expected.cpu.flags = completed.state.cpu.flags;
            }
            assert_eq!(completed.state, expected);
            assert_eq!(completed.exit, Exit::Dispatch(0x2000));
            assert_eq!(completed.dispatches, [(0x2000, completed.state.clone())]);
            assert_eq!(fault.state, completed.state);
            assert_eq!(
                fault.exit,
                Exit::PageFault {
                    address: 0x2000,
                    error: 0x10
                }
            );
            assert!(fault.dispatches.is_empty());
            assert!(completed.machine_unchanged && fault.machine_unchanged);
        }
    }
}

test_cases!(
    word_prefixes_code_boundaries_and_retirement_wrap,
    boundary_cases()
);
