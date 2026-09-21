use super::{flags, record, OPERATIONS};
use crate::support::encoding::check_length;
use crate::support::{
    cases::{
        test_cases, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    guest::{Exit, Machine},
    machine::Image,
    step::{Argument, Engine, Event, Input, Observation, Outcome, Snapshot, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError,
    Gpr32::{Eax, Ecx, Edi, Esi},
};

fn prefix_boundaries() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in OPERATIONS {
        for byte in [false, true] {
            for count in [0, 1, 2, 14] {
                let opcode = operation.code(if byte { 1 } else { 4 })[0];
                let code = [vec![0x66; count], vec![opcode]].concat();
                let width = if byte {
                    1
                } else if count == 0 {
                    4
                } else {
                    2
                };
                for origin in [0x2000 - code.len() as u32, u32::MAX] {
                    let name =
                        format!("{operation:?}, byte {byte}, {count} prefixes, EIP {origin:08x}");
                    let mut case = if operation.compares() {
                        Case::replacing_flags(name, &code, flags(10))
                    } else {
                        Case::preserving_flags(name, &code)
                    };
                    case = case
                        .stored_flags(record(0xfe))
                        .at(origin)
                        .instruction_count(u32::MAX)
                        .initial_registers(&[(Eax, 0x7856_3412), (Ecx, 0)]);
                    if operation.uses_source_index() {
                        case = case.register(Esi, 0x5000 - width, 0x5000).memory(
                            0x5000 - width,
                            &[0x12, 0x34, 0x56, 0x78][..width as usize],
                            ReadOnly,
                        );
                    } else {
                        case = case.initial_register(Esi, 0x5000 - width);
                    }
                    if operation.uses_destination_index() {
                        let permissions = if operation.writes_memory() {
                            ReadWrite
                        } else {
                            ReadOnly
                        };
                        let input = if operation.writes_memory() {
                            [0xa5; 4]
                        } else {
                            [0x12, 0x34, 0x56, 0x78]
                        };
                        case = case.register(Edi, 0x7000 - width, 0x7000).memory(
                            0x7000 - width,
                            &input[..width as usize],
                            permissions,
                        );
                        if operation.writes_memory() {
                            case = case.expect_memory(
                                0x7000 - width,
                                &[0x12, 0x34, 0x56, 0x78][..width as usize],
                            );
                        }
                    } else {
                        case = case.initial_register(Edi, 0x7000 - width);
                    }
                    cases.push(case);
                }
            }
        }
    }
    cases
}

#[test]
fn each_string_opcode_completes_without_an_operand_or_following_byte() {
    for opcode in [0xa4, 0xa5, 0xa6, 0xa7, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf] {
        for prefixes in [0, 1, 2, 14] {
            let code = [vec![0x66; prefixes], vec![opcode]].concat();
            check_length(&code);
        }
        assert_eq!(
            compile_block_from_bytes(0x1000, &[vec![0x66; 15], vec![opcode]].concat(), 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1000 })
        );
    }
}

#[test]
fn unsupported_prefixes_stop_before_any_string_access() {
    for prefix in [0xf0, 0xf2] {
        for opcode in [0xa4, 0xa5, 0xa6, 0xa7, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf] {
            if prefix == 0xf2 && matches!(opcode, 0xa6 | 0xa7 | 0xae | 0xaf) {
                continue;
            }
            for code in [vec![prefix, opcode], vec![0x66, prefix, opcode]] {
                assert_eq!(
                    compile_block_from_bytes(0x1000, &code, 1).err(),
                    Some(BlockError::UnsupportedInstruction {
                        address: 0x1000,
                        opcode: prefix
                    })
                );
            }
        }
        let mut image = Image::new(&[0x66, prefix, 0xa5]);
        image.cpu.registers.esi = 0x9000;
        image.cpu.registers.edi = 0xa000;
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "unsupported prefix preserves CPU before missing operands",
            Exit::Other(0x0008_0000_0000_1000 | (u64::from(prefix) << 32)),
        );
    }
}

#[test]
fn rep_loads_remain_unsupported() {
    for opcode in [0xac, 0xad] {
        for code in [vec![0xf3, opcode], vec![0x66, 0xf3, opcode]] {
            assert_eq!(
                compile_block_from_bytes(0x1000, &code, 1).err(),
                Some(BlockError::UnsupportedInstruction {
                    address: 0x1000,
                    opcode: 0xf3
                })
            );
            let mut image = Image::new(&code);
            image.cpu.registers.ecx = 0;
            image.cpu.registers.esi = 0x9000;
            image.cpu.registers.edi = 0xa000;
            image.check_unchanged_exit(
                Engine::Wasmtime,
                TestModule::interpreter(),
                "unsupported REP family rejects even with zero count",
                Exit::Other(0x0008_0000_0000_1000 | (0xf3u64 << 32)),
            );
        }
    }
}

#[test]
fn missing_opcode_and_instruction_length_faults_precede_string_data_access() {
    for count in [1, 14, 15] {
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x2000 - count;
        image.cpu.registers.esi = 0x9000;
        image.cpu.registers.edi = 0xa000;
        image.data(0x4000 - count, &vec![0x66; count as usize]);
        let exit = if count == 15 {
            Exit::Other(0x0002_0000_0000_0000)
        } else {
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            }
        };
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "fetch prefix without opcode before string access",
            exit,
        );
    }
}

#[test]
fn a_completed_string_operation_survives_the_following_fetch_fault() {
    let observer = TestModule::new(&crate::state::compile_flag_observer().unwrap());
    for operation in OPERATIONS {
        for width in [1, 2, 4] {
            let code = operation.code(width);
            let mut machine = Machine::at(0x2000 - code.len() as u32, &code);
            machine.cpu.flags = record(0xfe);
            machine.cpu.instruction_count = 17;
            machine.cpu.registers.eax = 0x7856_3412;
            machine.cpu.registers.esi = 0x4000;
            machine.cpu.registers.edi = 0x6000;
            machine.memory(0x4000, &[0x12, 0x34, 0x56, 0x78], ReadOnly);
            machine.memory(
                0x6000,
                &if operation.writes_memory() {
                    [0xa5; 4]
                } else {
                    [0x12, 0x34, 0x56, 0x78]
                },
                ReadWrite,
            );
            let initial = machine.state();
            let results = machine.run_many(TestModule::interpreter(), Engine::Wasmtime, 2);
            let completed = &results[0];
            let fault = &results[1];
            let mut expected = initial.clone();
            expected.cpu.eip = 0x2000;
            expected.cpu.instruction_count = 18;
            if operation.uses_source_index() {
                expected.cpu.registers.esi += width;
            }
            if operation.uses_destination_index() {
                expected.cpu.registers.edi += width;
            }
            if operation.writes_memory() {
                expected
                    .memory
                    .write(0x6000, &[0x12, 0x34, 0x56, 0x78][..width as usize]);
            }
            if operation.compares() {
                let cpu = completed.state.cpu.to_bytes();
                assert_eq!(
                    observer.observe(&Input::new(&cpu), 1),
                    Observation {
                        events: vec![Event::Return {
                            outcome: Outcome::Returned(vec![
                                Argument::I32(0),
                                Argument::I32(1),
                                Argument::I32(0),
                                Argument::I32(1),
                                Argument::I32(0),
                                Argument::I32(0)
                            ]),
                            snapshot: Snapshot {
                                cpu: cpu.to_vec(),
                                guest: None
                            }
                        }],
                        guest_unchanged: true,
                        machine_unchanged: true,
                    }
                );
                let bytes = completed.state.cpu.flags.bytes;
                let initial_bytes = initial.cpu.flags.bytes;
                assert_eq!(
                    [
                        bytes.tf,
                        bytes.df,
                        bytes.nt,
                        bytes.ac,
                        bytes.id,
                        bytes.reserved
                    ],
                    [
                        initial_bytes.tf,
                        initial_bytes.df,
                        initial_bytes.nt,
                        initial_bytes.ac,
                        initial_bytes.id,
                        initial_bytes.reserved
                    ]
                );
                assert_eq!(
                    completed.state.cpu.flags.status_source.reserved,
                    initial.cpu.flags.status_source.reserved
                );
                // The logical result is asserted above; its storage form belongs to state.
                expected.cpu.flags = completed.state.cpu.flags;
            }
            assert_eq!(completed.state, expected, "{operation:?} {width} bytes");
            assert_eq!(completed.exit, Exit::Dispatch(0x2000));
            assert_eq!(completed.dispatches, [(0x2000, expected)]);
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
    operand_prefixes_code_boundaries_and_count_wrap,
    prefix_boundaries()
);
