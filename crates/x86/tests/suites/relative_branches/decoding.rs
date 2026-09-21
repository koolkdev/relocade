use crate::support::encoding::check_length;
use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::machine::Image;

fn image_at(start: u32, code: &[u8]) -> Image {
    let mut image = Image::new(&[]);
    image.cpu.eip = start;
    image.guest.clear();
    image.machine.clear();
    let mut address = start;
    let mut remaining = code;
    let mut frame = 0x3000;
    while !remaining.is_empty() {
        let offset = address & 0xfff;
        let length = remaining.len().min((0x1000 - offset) as usize);
        image.map(address >> 12, frame, false);
        image.data(frame + offset, &remaining[..length]);
        address = address.wrapping_add(length as u32);
        remaining = &remaining[length..];
        frame += 0x2000;
    }
    image
}

use crate::support::{
    machine::{check, Exit, Step},
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
        let complete = check_length(code);
        let mut trailing = code.to_vec();
        trailing.extend_from_slice(&[0xb8, 0, 0, 0, 0, 0xf4]);
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
                image.cpu.flags.status_source.kind = 0;
                image.cpu.flags.bytes.zf = zero;
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
fn prefix_bytes_count_toward_the_branch_length_limit() {
    for (prefixes, suffix) in [
        (13, &[0xeb, 1][..]),
        (12, &[0xe9, 1, 0]),
        (13, &[0x74, 1]),
        (11, &[0x0f, 0x84, 1, 0]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        let overlong = [vec![0x66], code].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &overlong, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
        for zero in [0, 1] {
            let mut image = image_at(0x1ff1, &overlong[..15]);
            image.cpu.flags.status_source.kind = 0;
            image.cpu.flags.bytes.zf = zero;
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
