use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{self, both, Exit, Image, Step},
    step::TestModule,
};

#[path = "relative_branches/conditions.rs"]
mod conditions;
#[path = "relative_branches/decoding.rs"]
mod decoding;

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

#[test]
fn jumps_add_signed_displacements_to_the_end_of_the_instruction() {
    for (start, code, target) in [
        (0x1000, &[0xeb, 0][..], 0x1002),
        (0x1000, &[0xeb, 0x7f], 0x1081),
        (0x1000, &[0xeb, 0x80], 0x0f82),
        (0x1000, &[0xeb, 0xfe], 0x1000),
        (0, &[0xeb, 0x80], 0xffff_ff82),
        (0xffff_fffe, &[0xeb, 0x7f], 0x7f),
        (0x1000, &[0xe9, 0, 0, 0, 0], 0x1005),
        (0x1000, &[0xe9, 0xff, 0xff, 0xff, 0x7f], 0x8000_1004),
        (0x1000, &[0xe9, 0, 0, 0, 0x80], 0x8000_1005),
        (0, &[0xe9, 0xfa, 0xff, 0xff, 0xff], 0xffff_ffff),
        (0xffff_fffc, &[0xe9, 0, 0, 0, 0], 1),
        (0x1234_fffe, &[0x66, 0xeb, 0], 1),
        (0x1234_1000, &[0x66, 0xeb, 0x80], 0x0f83),
        (0x1234_1000, &[0x66, 0xe9, 0, 0], 0x1004),
        (0x1234_1000, &[0x66, 0xe9, 0xff, 0x7f], 0x9003),
        (0x1234_1000, &[0x66, 0xe9, 0, 0x80], 0x9004),
        (0x1234_fffe, &[0x66, 0xe9, 0xfd, 0xff], 0xffff),
        (0x1234_1000, &[0x66, 0x66, 0xe9, 0xfb, 0xff], 0x1000),
    ] {
        let mut image = image_at(start, code);
        // An unconditional transfer has no reason to inspect the flags record.
        image.cpu.flags.kind = 0xff;
        let mut cpu = image.cpu;
        cpu.eip = target;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            &format!("JMP {code:02x?} at {start:#x}"),
            code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(target),
            }],
        );
    }
}

#[test]
fn conditional_word_targets_truncate_only_when_the_branch_is_taken() {
    for (start, code, taken_target, fallthrough) in [
        (0x1234_fffe, &[0x66, 0x74, 0][..], 1, 0x1235_0001),
        (0x1234_1000, &[0x66, 0x74, 0x80], 0x0f83, 0x1234_1003),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0, 0][..],
            0x1005,
            0x1234_1005,
        ),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0xff, 0x7f],
            0x9004,
            0x1234_1005,
        ),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0, 0x80],
            0x9005,
            0x1234_1005,
        ),
        (
            0x1234_fffe,
            &[0x66, 0x0f, 0x84, 0xfc, 0xff],
            0xffff,
            0x1235_0003,
        ),
        (0xffff_fffd, &[0x66, 0x0f, 0x84, 0xfd, 0xff], 0xffff, 2),
    ] {
        for (zero, target) in [(0, fallthrough), (1, taken_target)] {
            let mut image = image_at(start, code);
            image.cpu.flags.kind = 0;
            image.cpu.flags.status.zf = zero;
            let mut cpu = image.cpu;
            cpu.eip = target;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                &format!("word JE {code:02x?} at {start:#x}, ZF={zero}"),
                code,
                1,
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
fn snapshot_limits_stop_at_the_first_branch_and_preserve_earlier_progress() {
    for suffix in [
        &[0xeb, 0x7f][..],
        &[0x74, 0x7f],
        &[0x0f, 0x84, 0x7f, 0, 0, 0],
    ] {
        for zero in [0, 1] {
            let mut code = vec![0xb8, 0x78, 0x56, 0x34, 0x12];
            code.extend_from_slice(suffix);
            let branch_end = 0x1000 + code.len() as u32;
            let target = if suffix[0] == 0xeb || zero != 0 {
                branch_end + 0x7f
            } else {
                branch_end
            };
            let mut image = Image::new(&code);
            image.cpu.flags.kind = 0;
            image.cpu.flags.status.zf = zero;
            image.cpu.instruction_count = 0xffff_fffe;
            let mut cpu = image.cpu;
            cpu.registers.eax = 0x1234_5678;
            cpu.eip = 0x1005;
            cpu.instruction_count = 0xffff_ffff;
            let first = Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(0x1005),
            };
            cpu.eip = target;
            cpu.instruction_count = 0;
            let last = Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(target),
            };
            // Even an untaken condition ends the snapshot block. A large limit
            // must not decode trailing bytes or require more snapshot input.
            both(
                TestModule::interpreter(),
                "branch ends a partial block",
                &code,
                u32::MAX,
                &image,
                &[first, last],
            );
            let module = compile_block_from_bytes(0x1000, &code, 2).unwrap();
            code.extend_from_slice(&[0x62, 0x66, 0x0f]);
            assert_eq!(
                compile_block_from_bytes(0x1000, &code, u32::MAX)
                    .unwrap()
                    .bytes,
                module.bytes
            );
            let bounded = compile_block_from_bytes(0x1000, &code, 1).unwrap();
            machine::check(
                &TestModule::new(&bounded),
                "limit before branch",
                &image,
                &[Step {
                    cpu: {
                        let mut cpu = image.cpu;
                        cpu.registers.eax = 0x1234_5678;
                        cpu.eip = 0x1005;
                        cpu.instruction_count = 0xffff_ffff;
                        cpu
                    },
                    ram: &[],
                    exit: Exit::Dispatch(0x1005),
                }],
            );
        }
    }
    assert!(matches!(
        compile_block_from_bytes(0x1000, &[0xeb, 0], 0),
        Err(BlockError::ZeroInstructionLimit)
    ));
    assert!(matches!(
        compile_block_from_bytes(0x1000, &[0xb0, 7], 2),
        Err(BlockError::TruncatedInstruction {
            address: 0x1002,
            available: 0
        })
    ));
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn branches_execute_in_optimizing_v8() {
    let code = [0x83, 0xe8, 1, 0x75, 0xfb]; // SUB EAX,1; JNE back to SUB.
    for initial in [1, 2] {
        let mut image = Image::new(&code);
        image.cpu.registers.eax = initial;
        let mut cpu = image.cpu;
        cpu.registers.eax = initial - 1;
        cpu.flags.kind = 9;
        cpu.flags.left = initial;
        cpu.flags.right = 1;
        cpu.eip = 0x1003;
        cpu.instruction_count = 0;
        let first = Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(0x1003),
        };
        cpu.eip = if initial == 1 { 0x1005 } else { 0x1000 };
        cpu.instruction_count = 1;
        let last = Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        };
        assert_eq!(
            TestModule::interpreter().observe_v8(&image.input(), 2),
            machine::expected(&image, &[first, last])
        );
        let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 8).unwrap());
        assert_eq!(
            module.observe_v8(&image.input(), 1),
            machine::expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip)
                }]
            )
        );
    }
}
