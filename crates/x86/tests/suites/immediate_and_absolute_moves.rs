#[path = "immediate_and_absolute_moves/sequences.rs"]
mod sequences;

#[path = "immediate_and_absolute_moves/cases.rs"]
mod cases;

use wasm86_x86::{compile_block_from_bytes, BlockError};
use wasmparser::Validator;

use crate::support::machine;
use crate::support::step;
use machine::{check, Exit, Step};
use step::TestModule;

use machine::byte_register_image as image;

#[test]
fn selected_form_lengths_include_the_address_then_the_immediate() {
    for code in [
        &[0xc6, 0xc4, 0x80][..],
        &[0xc7, 0xc0, 0xa0, 0x66, 0xc7, 0x88][..],
        &[0xc6, 0x44, 0x8b, 0x7f, 0xff][..],
        &[0xc7, 0x84, 0x8b, 0, 0x40, 0, 0, 0xa0, 0x66, 0xc7, 0x88][..],
        &[0xa0, 0x20, 0x40, 0, 0x80][..],
        &[0xa1, 0x20, 0x40, 0, 0x80][..],
        &[0xa2, 0x20, 0x40, 0, 0x80][..],
        &[0xa3, 0x20, 0x40, 0, 0x80][..],
    ] {
        for available in 0..code.len() {
            assert!(matches!(
                compile_block_from_bytes(0x1000, &code[..available], 1),
                Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                    if actual == available
            ));
        }
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut with_suffix = code.to_vec();
        with_suffix.extend_from_slice(&[0xc7, 0x0c]);
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            module.bytes
        );
    }
}

#[test]
fn unsupported_group_extensions_do_not_require_an_address_or_immediate() {
    // These ModRM bytes would require a SIB or disp32 if the group matched.
    for (opcode, modrm) in [(0xc6, 0x0c), (0xc7, 0x3d)] {
        for suffix in [&[][..], &[0x24, 0, 0, 0, 0, 0x80][..]] {
            let mut code = vec![opcode, modrm];
            code.extend_from_slice(suffix);
            assert!(matches!(
                compile_block_from_bytes(0x1ffe, &code, 1),
                Err(BlockError::UnsupportedInstruction { address: 0x1ffe, opcode: actual })
                    if actual == opcode
            ));
        }
    }
}

#[test]
fn encoding_fetches() {
    let step = TestModule::interpreter();
    for (name, start, available) in [
        ("missing grouped ModRM", 0x1fff, &[0xc6][..]),
        ("missing grouped displacement", 0x1ffe, &[0xc7, 0x05][..]),
        (
            "missing immediate wins over a missing data page",
            0x1ff9,
            &[0xc7, 0x05, 0, 0x40, 0, 0, 0xa0][..],
        ),
        (
            "missing absolute offset wins over a missing data page",
            0x1ffc,
            &[0xa0, 0, 0x40, 0][..],
        ),
    ] {
        let mut missing = image(&[]);
        missing.cpu.eip = start;
        missing.data(0x3000 + (start & 0xfff), available);
        let expected_cpu = missing.cpu;
        check(
            step,
            name,
            &missing,
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
    for (opcode, modrm) in [(0xc6, 0x0c), (0xc7, 0x3d)] {
        let mut unsupported = image(&[]);
        unsupported.cpu.eip = 0x1ffe;
        unsupported.data(0x3ffe, &[opcode, modrm]);
        let expected_cpu = unsupported.cpu;
        check(
            step,
            "unsupported group does not fetch the inaccessible address tail",
            &unsupported,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::Other((8 << 48) | ((opcode as u64) << 32) | 0x1ffe),
            }],
        );
    }
}
