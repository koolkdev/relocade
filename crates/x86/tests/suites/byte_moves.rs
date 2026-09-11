#[path = "byte_moves/sequences.rs"]
mod sequences;

use wasm86_x86::{compile_block_from_bytes, BlockError};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

use crate::support::machine;
use crate::support::step;
use machine::{check, Exit, Step};
use step::TestModule;

use machine::byte_register_image as byte_image;

#[path = "byte_moves/memory.rs"]
mod memory;
#[path = "byte_moves/registers.rs"]
mod registers;

#[test]
fn byte_forms_require_only_the_selected_encoding_bytes() {
    for bytes in [
        &[0xb0][..],
        &[0x88][..],
        &[0x8a, 0x04][..],
        &[0x88, 0x85, 0, 0, 0][..],
    ] {
        assert!(matches!(
            compile_block_from_bytes(0x1000, bytes, 1),
            Err(BlockError::TruncatedInstruction { address: 0x1000, available }) if available == bytes.len()
        ));
    }
    let prefix = [0xb4, 0x88, 0x8a, 0xc4];
    let expected = compile_block_from_bytes(0x1000, &prefix, 2).unwrap();
    for suffix in [&[0xb7][..], &[0x88][..], &[0x66, 0xb0, 0][..]] {
        let mut code = prefix.to_vec();
        code.extend_from_slice(suffix);
        assert_eq!(
            compile_block_from_bytes(0x1000, &code, 2).unwrap().bytes,
            expected.bytes
        );
    }
}

#[test]
fn byte_memory_moves_use_byte_guest_accesses() {
    for (code, expected_loads, expected_stores) in
        [(&[0x8a, 0x23][..], 1, 0), (&[0x88, 0x23][..], 0, 1)]
    {
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut guest = None;
        let mut memory_index = 0;
        let mut loads = 0;
        let mut stores = 0;
        for payload in Parser::new(0).parse_all(&module.bytes) {
            match payload.unwrap() {
                Payload::ImportSection(section) => {
                    for import in section {
                        let import = import.unwrap();
                        if matches!(import.ty, TypeRef::Memory(_)) {
                            if import.name == "guest" {
                                guest = Some(memory_index);
                            }
                            memory_index += 1;
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    for operation in body.get_operators_reader().unwrap() {
                        match operation.unwrap() {
                            Operator::I32Load8U { memarg } if Some(memarg.memory) == guest => {
                                loads += 1
                            }
                            Operator::I32Store8 { memarg } if Some(memarg.memory) == guest => {
                                stores += 1
                            }
                            Operator::I32Load { memarg } | Operator::I32Store { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                panic!("byte MOV accessed a guest dword")
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(guest.is_some());
        assert_eq!((loads, stores), (expected_loads, expected_stores));
    }
}

#[test]
fn instruction_boundaries() {
    let step = TestModule::interpreter();
    for (name, start, code) in [
        ("missing immediate", 0x1fff, &[0xb7][..]),
        ("missing ModRM", 0x1fff, &[0x8a][..]),
        (
            "missing SIB before byte data access",
            0x1ffe,
            &[0x88, 0x04][..],
        ),
    ] {
        let mut image = byte_image(&[]);
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
