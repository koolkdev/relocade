#[path = "word_moves/sequences.rs"]
mod sequences;

#[path = "word_moves/memory.rs"]
mod memory;
#[path = "word_moves/registers.rs"]
mod registers;

use wasm86_x86::{compile_block_from_bytes, Gpr32};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

// Word views occupy the low two bytes of each complete architectural register.
const REGISTERS: [(&str, Gpr32, u32); 8] = [
    ("AX", Gpr32::Eax, 0x4433_2211),
    ("CX", Gpr32::Ecx, 0x8877_6655),
    ("DX", Gpr32::Edx, 0xccbb_aa99),
    ("BX", Gpr32::Ebx, 0x10ff_eedd),
    ("SP", Gpr32::Esp, 0x7654_3210),
    ("BP", Gpr32::Ebp, 0xfedc_ba98),
    ("SI", Gpr32::Esi, 0x0123_4567),
    ("DI", Gpr32::Edi, 0x89ab_cdef),
];

#[test]
fn word_memory_moves_use_word_guest_accesses() {
    for (code, expected_loads, expected_stores) in [
        (&[0x66, 0x8b, 0x03][..], 1, 0),
        (&[0x66, 0x89, 0x03][..], 0, 1),
    ] {
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
                            Operator::I32Load16U { memarg } if Some(memarg.memory) == guest => {
                                loads += 1;
                            }
                            Operator::I32Store16 { memarg } if Some(memarg.memory) == guest => {
                                stores += 1;
                            }
                            Operator::I32Load { memarg } | Operator::I32Store { memarg }
                                if Some(memarg.memory) == guest =>
                            {
                                panic!("word MOV accessed a guest dword");
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
