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
