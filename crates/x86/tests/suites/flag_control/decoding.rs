use super::{operation_case, ENCODINGS};
use crate::support::{
    cases::{test_cases, InstructionCase as Case},
    machine::{check, Exit, Image, Step},
    step::TestModule,
};
use wasm86_x86::{compile_block_from_bytes, BlockError};
use wasmparser::Validator;

#[test]
fn opcode_completion_needs_no_operand_or_successor_byte() {
    for (_, opcode) in ENCODINGS {
        for prefixes in [0, 1, 2, 14] {
            let mut code = vec![0x66; prefixes];
            code.push(opcode);
            for available in 0..code.len() {
                assert!(matches!(
                    compile_block_from_bytes(0x1000, &code[..available], 1),
                    Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                        if actual == available
                ));
            }
            let complete = compile_block_from_bytes(0x1000, &code, 1).unwrap();
            Validator::new().validate_all(&complete.bytes).unwrap();
            code.push(0x0f);
            assert_eq!(
                compile_block_from_bytes(0x1000, &code, 1).unwrap().bytes,
                complete.bytes
            );
        }
    }
}

fn boundary_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, opcode) in ENCODINGS {
        for prefixes in [0, 1, 14] {
            let mut code = vec![0x66; prefixes];
            code.push(opcode);
            for origin in [0x2000 - code.len() as u32, u32::MAX] {
                cases.push(
                    operation_case(
                        format!("{name}, {prefixes} operand prefixes, at {origin:08x}"),
                        &code,
                        opcode,
                    )
                    .at(origin)
                    .instruction_count(u32::MAX),
                );
            }
        }
    }
    cases
}

#[test]
fn unsupported_prefixes_stop_before_the_flag_control_opcode() {
    for prefix in [0xf0, 0xf2, 0xf3] {
        for (_, opcode) in ENCODINGS {
            for code in [vec![prefix, opcode], vec![0x66, prefix, opcode]] {
                assert!(matches!(
                    compile_block_from_bytes(0x1000, &code, 1),
                    Err(BlockError::UnsupportedInstruction { address: 0x1000, opcode: actual })
                        if actual == prefix
                ));
                let image = Image::new(&code);
                check(
                    TestModule::interpreter(),
                    "unsupported prefix preserves the complete CPU and memory",
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
fn the_length_limit_precedes_the_sixteenth_opcode_fetch() {
    for (_, opcode) in ENCODINGS {
        let code = [vec![0x66; 15], vec![opcode]].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
    }
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x1ff1;
    image.data(0x3ff1, &[0x66; 15]);
    check(
        TestModule::interpreter(),
        "fifteen prefixes preserve flags before the absent opcode page",
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::Other(0x0002_0000_0000_0000),
        }],
    );
}

#[test]
fn a_missing_opcode_after_operand_prefixes_leaves_all_flags_unchanged() {
    for count in [1, 14] {
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x2000 - count;
        image.data(0x4000 - count, &vec![0x66; count as usize]);
        check(
            TestModule::interpreter(),
            "opcode fetch fault preserves the complete incoming state",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            }],
        );
    }
}

#[test]
fn completed_direction_changes_survive_the_next_instruction_fetch_fault() {
    for (opcode, direction) in [(0xfc, 0), (0xfd, 1)] {
        for prefixes in [0, 1, 14] {
            let mut code = vec![0x66; prefixes];
            code.push(opcode);
            let mut image = Image::new(&[]);
            image.cpu.eip = 0x2000 - code.len() as u32;
            image.data(0x4000 - code.len() as u32, &code);
            let mut completed = image.cpu;
            completed.flags.bytes.df = direction;
            completed.eip = 0x2000;
            completed.instruction_count = 0;
            check(
                TestModule::interpreter(),
                "completed DF write remains visible at the following fetch fault",
                &image,
                &[
                    Step {
                        cpu: completed,
                        ram: &[],
                        exit: Exit::Dispatch(0x2000),
                    },
                    Step {
                        cpu: completed,
                        ram: &[],
                        exit: Exit::PageFault {
                            address: 0x2000,
                            error: 0x10,
                        },
                    },
                ],
            );
        }
    }
}

test_cases!(prefix_budget_page_end_and_wrapped_eip, boundary_cases());
