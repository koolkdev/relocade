use crate::support::config::{FLAT32, REAL16, SEGMENTED16, SEGMENTED32};
use crate::Compiler;
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
    for config in [FLAT32, SEGMENTED32, SEGMENTED16, REAL16] {
        let compiler = Compiler::new(config);
        for code in [&[0x40][..], &[0x8d, 0x00][..]] {
            // INC and LEA need CPU state but do not access guest memory.
            let block = compiler.compile_block(0x1000, code, 1).unwrap();
            assert_eq!(
                imports(&block.bytes),
                [("relocade", "executionContext")],
                "{config:?}, {code:02x?}",
            );
        }
        for code in [&[0x50][..], &[0x8b, 0x00][..]] {
            // PUSH uses memory without a memory operand; MOV has an explicit one.
            let block = compiler.compile_block(0x1000, code, 1).unwrap();
            let map = match config.memory_backend {
                crate::MemoryBackend::PhysicalBus => "physicalMap",
                crate::MemoryBackend::HostPageTable => "machine",
            };
            assert_eq!(
                imports(&block.bytes),
                [
                    ("relocade", "executionContext"),
                    ("relocade", "guest"),
                    ("relocade", map),
                ],
                "{config:?}, {code:02x?}",
            );
        }
    }
}

#[test]
fn all_register_stack_transfers_reuse_one_span_check() {
    use wasmparser::Operator;

    for (opcode, loads, stores) in [(0x60, 0, 8), (0x61, 7, 0)] {
        let block = Compiler::new(FLAT32)
            .compile_block(0x1000, &[opcode], 1)
            .unwrap();
        let mut memories = Vec::new();
        let mut page_reads = 0;
        let mut guest_loads = 0;
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
                            Operator::I32Load { memarg }
                                if memories[memarg.memory as usize] == "guest" =>
                            {
                                guest_loads += 1;
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
        // At most two page checks serve all transfers; POPAD discards the ESP slot.
        assert_eq!(page_reads, 2, "opcode {opcode:02x}");
        assert_eq!(guest_loads, loads, "opcode {opcode:02x}");
        assert_eq!(guest_stores, stores, "opcode {opcode:02x}");
    }
}

#[test]
fn physical_jit_operands_do_not_embed_general_transfer_helpers() {
    let compiler = Compiler::new(REAL16);
    // Scalar load, read/modify/write, and a repeated copy all retain direct proofs.
    for code in [&[0xa1, 0, 0x20][..], &[0xff, 0x06, 0, 0x20], &[0xf3, 0xa4]] {
        let block = compiler.compile_block(0x1000, code, 1).unwrap();
        for payload in Parser::new(0).parse_all(&block.bytes) {
            if let Payload::ImportSection(imports) = payload.unwrap() {
                for import in imports {
                    let import = import.unwrap();
                    assert!(
                        !matches!(import.name, "mmioReads" | "mmioWrites" | "writeCode"),
                        "{code:02x?} must hand off before a host memory transfer",
                    );
                }
            }
        }
    }
}
