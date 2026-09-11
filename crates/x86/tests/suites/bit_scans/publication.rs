//! Scan publication leaves discarded arithmetic operands in their existing backing.
//! Architectural scan results and flags are checked by the ordinary sequences.

use wasm86_x86::{compile_block_from_bytes, CpuState, StatusFlags, StoredFlags};

use super::image;
use crate::support::{
    machine::{expected, Exit, Step},
    step::{Engine, TestModule},
};

fn check_discarded_payload(engine: Engine) {
    for opcode in [0xbc, 0xbd] {
        let code = [
            0x80, 0xc3, 1, // ADD BL,1 publishes operands only at an interpreter boundary.
            0x66, 0x0f, opcode, 0xc2, // BSF/BSR AX,DX replaces the arithmetic flags.
            0x0f, 0xbd, 0x6d, 0, // BSR EBP,[EBP] faults without publishing another result.
        ];
        let mut image = image(&code);
        image.cpu.registers.ebx = 0xccbb_00ff;
        image.cpu.registers.edx = 0;
        image.cpu.registers.ebp = 0x5001;
        let mut after_add = CpuState {
            eip: 0x1003,
            instruction_count: 0,
            flags: StoredFlags {
                kind: 2,
                left: 0xff,
                right: 1,
                ..image.cpu.flags
            },
            ..image.cpu
        };
        after_add.registers.ebx = 0xccbb_0000;
        let after_scan = CpuState {
            eip: 0x1007,
            instruction_count: 1,
            flags: StoredFlags {
                kind: 0,
                status: StatusFlags {
                    cf: 0,
                    pf: 1,
                    af: 0,
                    zf: 1,
                    sf: 0,
                    of: 0,
                },
                ..after_add.flags
            },
            ..after_add
        };
        let fault = Exit::PageFault {
            address: 0x5001,
            error: 0,
        };
        let steps = [
            Step {
                cpu: after_add,
                ram: &[],
                exit: Exit::Dispatch(0x1003),
            },
            Step {
                cpu: after_scan,
                ram: &[],
                exit: Exit::Dispatch(0x1007),
            },
            Step {
                cpu: after_scan,
                ram: &[],
                exit: fault,
            },
        ];
        assert_eq!(
            engine.observe(TestModule::interpreter(), &image.input(), 3),
            expected(&image, &steps),
            "{engine:?}, scan opcode {opcode:x}: interpreter retains published ADD operands"
        );
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 3).unwrap());
        let block_cpu = CpuState {
            flags: StoredFlags {
                left: 0x1234_5678,
                right: 0x8765_4321,
                ..after_scan.flags
            },
            ..after_scan
        };
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: block_cpu,
                    ram: &[],
                    exit: fault
                }]
            ),
            "{engine:?}, scan opcode {opcode:x}: block preserves unused incoming payload"
        );
    }
}

#[test]
fn scans_preserve_unused_payload_at_their_publication_boundaries() {
    check_discarded_payload(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn scan_payload_publication_in_v8() {
    check_discarded_payload(Engine::V8);
}
