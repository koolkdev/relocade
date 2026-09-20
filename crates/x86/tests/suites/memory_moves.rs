#[path = "memory_moves/sequences.rs"]
mod sequences;

use wasm86_x86::{compile_block_from_bytes, BlockError};
use wasmparser::Validator;

use crate::support::step;
use step::TestModule;

use crate::support::machine;
use machine::{check, Exit, Image, Step};

#[path = "memory_moves/cases.rs"]
mod cases;

#[path = "memory_moves/addresses.rs"]
mod addresses;

#[test]
fn selected_memory_forms_require_only_their_address_bytes() {
    for bytes in [
        &[0x8b, 0x04][..],
        &[0x8b, 0x44, 0x24],
        &[0x89, 0x05, 0x12, 0x34, 0x56],
    ] {
        assert_eq!(
            compile_block_from_bytes(0x1000, bytes, 1).err(),
            Some(BlockError::TruncatedInstruction {
                address: 0x1000,
                available: bytes.len()
            })
        );
    }
    let code = [0x8b, 0x14, 0x25, 0xf3, 0x0f, 0xb8, 0x66, 0xf4];
    let module = compile_block_from_bytes(0x1000, &code, 1).unwrap();
    Validator::new().validate_all(&module.bytes).unwrap();
    assert_eq!(
        compile_block_from_bytes(0x1000, &code, 2).err(),
        Some(BlockError::UnsupportedInstruction {
            address: 0x1007,
            opcode: 0xf4
        })
    );
}

#[test]
fn missing_address_fields_fault_before_data_access() {
    let step = TestModule::interpreter();
    for (name, start, code) in [
        ("missing SIB before data access", 0x1ffe, &[0x8b, 0x04][..]),
        (
            "missing disp8 before data access",
            0x1ffe,
            &[0x8b, 0x40][..],
        ),
        (
            "missing disp32 before data access",
            0x1ffc,
            &[0x8b, 0x80, 0x12, 0x34][..],
        ),
    ] {
        let mut image = Image::new(&[]);
        image.cpu.eip = start;
        image.cpu.registers.eax = 0x4000;
        image.data(0x3000 + (start & 0xfff), code);
        let expected_cpu = image.cpu;
        check(
            step,
            name,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x00002000,
                    error: 0x10,
                },
            }],
        );
    }
}
