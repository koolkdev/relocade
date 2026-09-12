#[path = "relative_branches/cases.rs"]
mod cases;
#[path = "relative_branches/sequences.rs"]
mod sequences;

use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{self, both, Exit, Image, Step},
    step::TestModule,
};

#[path = "relative_branches/conditions.rs"]
mod conditions;
#[path = "relative_branches/decoding.rs"]
mod decoding;

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
            image.cpu.flags.status_source.kind = 0;
            image.cpu.flags.bytes.zf = zero;
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
