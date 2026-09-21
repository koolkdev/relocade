use super::{byte_flags, concrete_record, sahf_flags, Case, Flags, Preserved};
use crate::support::encoding::check_length;
use crate::support::{
    cases::test_cases,
    guest::{Exit, Machine},
    machine::Image,
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, Gpr32::Eax};

const OPCODES: [u8; 2] = [0x9e, 0x9f];

fn boundary_cases() -> Vec<Case> {
    let flags = byte_flags(0x93, true);
    let mut cases = Vec::new();
    for opcode in OPCODES {
        for prefixes in [0, 1, 2, 14] {
            let code = [vec![0x66; prefixes], vec![opcode]].concat();
            for origin in [0x2000 - code.len() as u32, u32::MAX] {
                let case = Case::new(
                    format!("{opcode:02x}, {prefixes} operand prefixes, EIP {origin:08x}"),
                    &code,
                    flags,
                    if opcode == 0x9f {
                        Flags::all(Preserved)
                    } else {
                        sahf_flags(0x6c)
                    },
                )
                .stored_flags(concrete_record(flags, 0xfe))
                .register(
                    Eax,
                    0x4433_6c11,
                    if opcode == 0x9f {
                        0x4433_9311
                    } else {
                        0x4433_6c11
                    },
                )
                .at(origin)
                .instruction_count(u32::MAX);
                cases.push(if opcode == 0x9f {
                    case.preserve_flag_record()
                } else {
                    case
                });
            }
        }
    }
    cases
}

#[test]
fn the_opcode_completes_each_transfer_without_an_operand_or_next_byte() {
    for opcode in OPCODES {
        for prefixes in [0, 1, 2, 14] {
            let code = [vec![0x66; prefixes], vec![opcode]].concat();
            check_length(&code);
        }
    }
}

#[test]
fn unsupported_prefixes_leave_the_transfer_unexecuted() {
    for prefix in [0xf0, 0xf2, 0xf3] {
        for opcode in OPCODES {
            for code in [vec![prefix, opcode], vec![0x66, prefix, opcode]] {
                assert!(matches!(
                    compile_block_from_bytes(0x1000, &code, 1),
                    Err(BlockError::UnsupportedInstruction { address: 0x1000, opcode: actual })
                        if actual == prefix
                ));
                let image = Image::new(&code);
                image.check_unchanged_exit(
                    Engine::Wasmtime,
                    TestModule::interpreter(),
                    "unsupported prefix preserves the complete state",
                    Exit::Other(0x0008_0000_0000_1000 | (u64::from(prefix) << 32)),
                );
            }
        }
    }
}

#[test]
fn the_fifteen_byte_limit_precedes_fetching_a_transfer_opcode() {
    for opcode in OPCODES {
        let code = [vec![0x66; 15], vec![opcode]].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
    }
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x1ff1;
    image.data(0x3ff1, &[0x66; 15]);
    image.check_unchanged_exit(
        Engine::Wasmtime,
        TestModule::interpreter(),
        "the absent sixteenth byte is not fetched",
        Exit::Other(0x0002_0000_0000_0000),
    );
}

#[test]
fn missing_opcodes_after_prefixes_preserve_ah_and_flags() {
    for count in [1, 14] {
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x2000 - count;
        image.data(0x4000 - count, &vec![0x66; count as usize]);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "opcode fetch fault precedes any AH transfer",
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
}

#[test]
fn completed_transfers_remain_published_at_the_following_fetch_fault() {
    for opcode in OPCODES {
        for prefixes in [0, 1, 14] {
            let code = [vec![0x66; prefixes], vec![opcode]].concat();
            let mut machine = Machine::at(0x2000 - code.len() as u32, &code);
            let flags = byte_flags(0x93, true);
            machine.cpu.flags = concrete_record(flags, 0xfe);
            machine.cpu.registers.eax = 0x4433_4611;
            let initial = machine.state();
            let executions = machine.run_many(TestModule::interpreter(), Engine::Wasmtime, 2);
            let completed = &executions[0];
            let fault = &executions[1];
            assert_eq!(completed.exit, Exit::Dispatch(0x2000));
            assert_eq!(completed.dispatches, [(0x2000, completed.state.clone())]);
            let mut unchanged = completed.state.clone();
            unchanged.cpu.eip = initial.cpu.eip;
            unchanged.cpu.instruction_count = initial.cpu.instruction_count;
            if opcode == 0x9f {
                assert_eq!(completed.state.cpu.registers.eax, 0x4433_9311);
                unchanged.cpu.registers.eax = initial.cpu.registers.eax;
            } else {
                // Concrete incoming flags make these five published bytes directly observable.
                let bytes = completed.state.cpu.flags.bytes;
                assert_eq!(
                    [bytes.cf, bytes.pf, bytes.af, bytes.zf, bytes.sf],
                    [0, 1, 0, 1, 0]
                );
                assert_eq!(bytes.of & 1, 1);
                assert_eq!(
                    [
                        bytes.tf,
                        bytes.df,
                        bytes.nt,
                        bytes.ac,
                        bytes.id,
                        bytes.reserved
                    ],
                    [0xa5, 0xfe, 0xa5, 0xa5, 0xa5, 0xa5]
                );
                assert_eq!(
                    completed.state.cpu.flags.status_source,
                    initial.cpu.flags.status_source
                );
                unchanged.cpu.flags = initial.cpu.flags;
            }
            assert_eq!(unchanged, initial);
            assert_eq!(completed.state.cpu.eip, 0x2000);
            assert_eq!(completed.state.cpu.instruction_count, 0);
            assert_eq!(
                fault.exit,
                Exit::PageFault {
                    address: 0x2000,
                    error: 0x10
                }
            );
            assert!(fault.dispatches.is_empty());
            assert_eq!(fault.state, completed.state);
            assert!(completed.machine_unchanged && fault.machine_unchanged);
        }
    }
}

test_cases!(operand_prefixes_page_end_and_wrapped_eip, boundary_cases());
