use wasm86_x86::{compile_block_from_bytes, BlockError};

use super::image_at;
use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

const ENCODINGS: &[&[u8]] = &[
    &[0xeb, 1],
    &[0x66, 0xeb, 1],
    &[0xe9, 1, 0, 0, 0],
    &[0x66, 0xe9, 1, 0],
    &[0x74, 1],
    &[0x66, 0x74, 1],
    &[0x0f, 0x84, 1, 0, 0, 0],
    &[0x66, 0x0f, 0x84, 1, 0],
];

#[test]
fn snapshots_require_every_opcode_and_displacement_byte_before_ending_a_block() {
    for &code in ENCODINGS {
        for available in 0..code.len() {
            assert!(
                matches!(
                    compile_block_from_bytes(0x1000, &code[..available], 1),
                    Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual }) if actual == available
                ),
                "{code:02x?}, {available} bytes"
            );
        }
        let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let mut trailing = code.to_vec();
        trailing.extend_from_slice(&[0xb8, 0, 0, 0, 0, 0x62]);
        assert_eq!(
            compile_block_from_bytes(0x1000, &trailing, 99)
                .unwrap()
                .bytes,
            complete.bytes
        );
    }
}

#[test]
fn runtime_fetches_all_branch_fields_for_both_condition_outcomes() {
    for &code in ENCODINGS {
        for available in 1..code.len() {
            let start = 0x2000 - available as u32;
            // The untaken condition still requires the displacement.
            for zero in [0, 1] {
                let mut image = image_at(start, &code[..available]);
                image.cpu.flags.kind = 0;
                image.cpu.flags.status.zf = zero;
                check(
                    TestModule::interpreter(),
                    &format!("incomplete branch {code:02x?}, {available} bytes, ZF={zero}"),
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
    }
}

#[test]
fn complete_branches_dispatch_without_fetching_the_successor() {
    for &code in ENCODINGS {
        for zero in [0, 1] {
            let start = 0x2000 - code.len() as u32;
            let mut image = image_at(start, code);
            image.cpu.flags.kind = 0;
            image.cpu.flags.status.zf = zero;
            let conditional = code.contains(&0x74) || code.contains(&0x84);
            let target = if conditional && zero == 0 {
                0x2000
            } else {
                0x2001
            };
            let mut cpu = image.cpu;
            cpu.eip = target;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                &format!("successor absent after {code:02x?}, ZF={zero}"),
                code,
                8,
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(target),
                }],
            );
        }
    }
}

#[test]
fn prefix_bytes_count_toward_the_branch_length_limit() {
    for (prefixes, suffix) in [
        (13, &[0xeb, 1][..]),
        (12, &[0xe9, 1, 0]),
        (13, &[0x74, 1]),
        (11, &[0x0f, 0x84, 1, 0]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert_eq!(code.len(), 15);
        let mut image = image_at(0x1ff1, &code);
        image.cpu.flags.kind = 0;
        image.cpu.flags.status.zf = 1;
        let mut cpu = image.cpu;
        cpu.eip = 0x2001;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "fifteen-byte branch",
            &code,
            8,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(0x2001),
            }],
        );

        let overlong = [vec![0x66], code].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &overlong, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
        for zero in [0, 1] {
            let mut image = image_at(0x1ff1, &overlong[..15]);
            image.cpu.flags.kind = 0;
            image.cpu.flags.status.zf = zero;
            check(
                TestModule::interpreter(),
                "overlong branch faults before next-page fetch",
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0002_0000_0000_0000),
                }],
            );
        }
    }
}

#[test]
fn faults_before_a_branch_publish_only_completed_instructions() {
    // MOV EAX,7; MOV ECX,[4000]; JMP. The missing data page prevents the branch.
    let code = [0xb8, 7, 0, 0, 0, 0x8b, 0x0d, 0, 0x40, 0, 0, 0xeb, 0x7f];
    let image = image_at(0x1000, &code);
    let mut cpu = image.cpu;
    cpu.registers.eax = 7;
    cpu.eip = 0x1005;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        "memory fault before terminating branch",
        &code,
        8,
        &image,
        &[
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            },
            Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x4000,
                    error: 0,
                },
            },
        ],
    );

    // MOV AL,7 completes, then a JE has no displacement byte on the next page.
    let image = image_at(0x1ffd, &[0xb0, 7, 0x74]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x1111_1107;
    cpu.eip = 0x1fff;
    cpu.instruction_count = 0;
    check(
        TestModule::interpreter(),
        "branch fetch fault retains prior instruction",
        &image,
        &[
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(0x1fff),
            },
            Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            },
        ],
    );
}
