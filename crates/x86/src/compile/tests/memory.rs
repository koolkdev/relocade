use crate::{Compiler, ExecutionProfile, SegmentProfile};
use wasmparser::{Parser, Payload, TypeRef};

pub(super) fn imports(bytes: &[u8]) -> Vec<(&str, &str)> {
    let mut memories = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for import in imports {
                let import = import.unwrap();
                if matches!(import.ty, TypeRef::Memory(_)) {
                    memories.push((import.module, import.name));
                }
            }
        }
    }
    memories.sort_unstable();
    memories
}

#[test]
fn snapshot_imports_cover_explicit_and_implicit_memory_accesses() {
    for profile in [
        SegmentProfile::Flat32.into(),
        SegmentProfile::Segmented32.into(),
        SegmentProfile::Segmented16.into(),
        ExecutionProfile::Real16,
    ] {
        let compiler = Compiler::new(profile);
        for code in [&[0x40][..], &[0x8d, 0x00][..]] {
            // INC and LEA need CPU state but do not access guest memory.
            let block = compiler.compile_block(0x1000, code, 1).unwrap();
            assert_eq!(
                imports(&block.bytes),
                [("relocade", "executionContext")],
                "{profile:?}, {code:02x?}",
            );
        }
        for code in [&[0x50][..], &[0x8b, 0x00][..]] {
            // PUSH uses memory without a memory operand; MOV has an explicit one.
            let block = compiler.compile_block(0x1000, code, 1).unwrap();
            let map = match profile {
                ExecutionProfile::Real16 => "physicalMap",
                ExecutionProfile::Protected(_) => "machine",
            };
            assert_eq!(
                imports(&block.bytes),
                [
                    ("relocade", "executionContext"),
                    ("relocade", "guest"),
                    ("relocade", map),
                ],
                "{profile:?}, {code:02x?}",
            );
        }
    }
}

#[test]
fn pushad_reuses_one_span_check_for_all_eight_stores() {
    use wasmparser::Operator;

    let block = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &[0x60], 1)
        .unwrap();
    let mut memories = Vec::new();
    let mut page_reads = 0;
    let mut guest_stores = 0;
    for payload in Parser::new(0).parse_all(&block.bytes) {
        match payload.unwrap() {
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.unwrap();
                    if matches!(import.ty, TypeRef::Memory(_)) {
                        memories.push(import.name);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::I32Load { memarg }
                            if memories[memarg.memory as usize] == "machine" =>
                        {
                            page_reads += 1;
                        }
                        Operator::I32Store { memarg }
                            if memories[memarg.memory as usize] == "guest" =>
                        {
                            guest_stores += 1;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    // A frame touches at most two pages. Both are checked before its eight stores.
    assert_eq!(page_reads, 2);
    assert_eq!(guest_stores, 8);
}
