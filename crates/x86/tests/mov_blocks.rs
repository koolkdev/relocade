use std::{
    fmt::Write,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, CompiledModule};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, ValType, Validator};

// Intel SDM, MOV: B8+rd id copies imm32 into r32 and leaves flags unchanged.
// Offsets below are literal CPU/Wasm ABI expectations, independent of state helpers.
const SINGLE_MOVES: [(&[u8], usize, u32); 8] = [
    (&[0xb8, 0x78, 0x56, 0x34, 0x12], 24, 0x1234_5678),
    (&[0xb9, 0, 0, 0, 0x80], 28, 0x8000_0000),
    (&[0xba, 0xff, 0xff, 0xff, 0xff], 32, 0xffff_ffff),
    (&[0xbb, 0, 0, 0, 0], 36, 0),
    (&[0xbc, 0xf3, 0x0f, 0xb8, 0x66], 40, 0x66b8_0ff3),
    (&[0xbd, 0xff, 0xff, 0xff, 0x7f], 44, 0x7fff_ffff),
    (&[0xbe, 0xef, 0xbe, 0xad, 0xde], 48, 0xdead_beef),
    (&[0xbf, 0x21, 0x43, 0x65, 0x87], 52, 0x8765_4321),
];
const TWO: &[u8] = &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xbf, 0xff, 0xff, 0xff, 0xff];
const OVERWRITE: &[u8] = &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xb8, 0xff, 0xff, 0xff, 0xff];
const REVERSE: &[u8] = &[0xbf, 0xff, 0xff, 0xff, 0xff, 0xb8, 0x78, 0x56, 0x34, 0x12];
const FIRST_WRITE_ORDER: &[u8] = &[
    0xbf, 0x11, 0x11, 0x11, 0x11, 0xb8, 0x22, 0x22, 0x22, 0x22, 0xbf, 0x33, 0x33, 0x33, 0x33,
];

#[test]
fn selected_mov_requires_all_five_bytes() {
    let instruction = &[0xbf, 0x78, 0x56, 0x34, 0x12];
    for available in 0..5 {
        assert!(matches!(
            compile_block_from_bytes(0x1000, &instruction[..available], 1),
            Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual }) if actual == available
        ));
        let mut after_prefix = SINGLE_MOVES[0].0.to_vec();
        after_prefix.extend_from_slice(&instruction[..available]);
        assert!(matches!(
            compile_block_from_bytes(0xffff_fffd, &after_prefix, 2),
            Err(BlockError::TruncatedInstruction { address: 2, available: actual }) if actual == available
        ));
    }
}

#[test]
fn unsupported_selected_opcodes_report_their_instruction_address() {
    for bytes in [&[0x90][..], &[0x66, 0xb8, 0, 0, 0, 0][..]] {
        assert!(matches!(
            compile_block_from_bytes(0x1000, bytes, 1),
            Err(BlockError::UnsupportedInstruction { address: 0x1000, opcode }) if opcode == bytes[0]
        ));
    }
    let mut bytes = SINGLE_MOVES[0].0.to_vec();
    bytes.push(0x66);
    assert!(matches!(
        compile_block_from_bytes(0x1000, &bytes, 2),
        Err(BlockError::UnsupportedInstruction {
            address: 0x1005,
            opcode: 0x66
        })
    ));
}

#[test]
fn instruction_limit_excludes_valid_partial_and_unsupported_suffixes() {
    let expected = compile_block_from_bytes(0x1000, SINGLE_MOVES[0].0, 1).unwrap();
    for suffix in [&TWO[5..], &[0xbf, 0x12][..], &[0x66][..]] {
        let mut bytes = SINGLE_MOVES[0].0.to_vec();
        bytes.extend_from_slice(suffix);
        let block = compile_block_from_bytes(0x1000, &bytes, 1).unwrap();
        assert_eq!(block.bytes, expected.bytes);
        assert_eq!(block.entry, expected.entry);
    }
}

#[test]
fn completed_state_is_published_once_in_first_write_order_before_tail_dispatch() {
    let block = compile_block_from_bytes(0x1000, FIRST_WRITE_ORDER, 3).unwrap();
    Validator::new().validate_all(&block.bytes).unwrap();
    assert_eq!(block.entry, "block_1000");
    let mut types = Vec::new();
    let mut imports = Vec::new();
    let mut functions = Vec::new();
    let mut exports = Vec::new();
    let mut loads = Vec::new();
    let mut stores = Vec::new();
    let mut additions = 0;
    let mut tails = Vec::new();
    for payload in Parser::new(0).parse_all(&block.bytes) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                for ty in section.into_iter_err_on_gc_types() {
                    let ty = ty.unwrap();
                    types.push((ty.params().to_vec(), ty.results().to_vec()));
                }
            }
            Payload::ImportSection(section) => {
                for import in section {
                    let import = import.unwrap();
                    assert_eq!(import.module, "wasm86");
                    imports.push((import.name.to_owned(), import.ty));
                }
            }
            Payload::FunctionSection(section) => {
                functions.extend(section.into_iter().map(Result::unwrap))
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    exports.push((export.name.to_owned(), export.kind, export.index));
                }
            }
            Payload::CodeSectionEntry(body) => {
                let mut operators = body.get_operators_reader().unwrap();
                while !operators.eof() {
                    match operators.read().unwrap() {
                        Operator::I32Load { memarg } => {
                            assert_eq!(memarg.memory, 0);
                            loads.push(memarg.offset);
                        }
                        Operator::I32Store { memarg } => {
                            assert_eq!(memarg.memory, 0);
                            stores.push(memarg.offset);
                        }
                        Operator::I32Add => additions += 1,
                        Operator::ReturnCall { function_index } => tails.push(function_index),
                        Operator::Call { .. } | Operator::Return => {
                            panic!("dispatch must be a tail call")
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        types,
        [
            (vec![], vec![ValType::I64]),
            (vec![ValType::I32], vec![ValType::I64])
        ]
    );
    assert_eq!(functions, [0]);
    assert_eq!(exports, [("block_1000".to_owned(), ExternalKind::Func, 1)]);
    assert_eq!(imports.len(), 2);
    assert!(imports.iter().any(|(name, ty)| name == "cpuState" && matches!(ty, TypeRef::Memory(memory) if memory.initial == 1 && !memory.memory64 && !memory.shared)));
    assert!(imports
        .iter()
        .any(|(name, ty)| name == "dispatch" && matches!(ty, TypeRef::Func(1))));
    assert_eq!(loads, [144]);
    assert_eq!(stores, [52, 24, 56, 144]);
    assert_eq!(additions, 1);
    assert_eq!(tails, [0]);
}

fn state(count: u32) -> [u8; 152] {
    let mut bytes = [0xa5; 152];
    for (offset, value) in [
        (24, 0x1111_1111u32),
        (28, 0x2222_2222),
        (32, 0x3333_3333),
        (36, 0x4444_4444),
        (40, 0x5555_5555),
        (44, 0x6666_6666),
        (48, 0x7777_7777),
        (52, 0x8888_8888),
        (56, 0xdead_beef),
        (144, count),
        (148, 0x1234_5678),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut text, "{byte:02x}").unwrap();
    }
    text
}

struct ModuleFile(PathBuf);
impl ModuleFile {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-mov-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        Self(path)
    }
}
impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn check(
    flags: &[&str],
    block: &CompiledModule,
    initial: &[u8],
    updates: &[(usize, u32)],
    dispatched: i32,
    returned: i64,
) {
    let module = ModuleFile::new(&block.bytes);
    let mut expected = initial.to_vec();
    for (offset, value) in updates {
        expected[*offset..*offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let expected = hex(&expected);
    let output = Command::new("node")
        .args(flags)
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/execute-block.mjs"
        ))
        .arg(&module.0)
        .arg(&block.entry)
        .arg(hex(initial))
        .arg(returned.to_string())
        .output()
        .expect("the explicit V8 lane requires Node.js on PATH");
    assert!(
        output.status.success(),
        "entry {}: {}",
        block.entry,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("dispatch({dispatched}) {expected}\nreturn {returned}\nstate {expected}\n"),
        "entry {}, updates {updates:?}, V8 flags {flags:?}",
        block.entry
    );
}

fn check_execution(flags: &[&str]) {
    let initial = state(u32::MAX);
    for &(bytes, offset, value) in &SINGLE_MOVES {
        let block = compile_block_from_bytes(0x1000, bytes, 1).unwrap();
        check(
            flags,
            &block,
            &initial,
            &[(offset, value), (56, 0x1005), (144, 0)],
            4101,
            i64::MIN,
        );
    }
    for (bytes, limit, updates, next) in [
        (
            TWO,
            2,
            &[(24, 0x1234_5678), (52, 0xffff_ffff), (56, 0x100a), (144, 1)][..],
            4106,
        ),
        (
            OVERWRITE,
            2,
            &[(24, 0xffff_ffff), (56, 0x100a), (144, 1)][..],
            4106,
        ),
        (
            REVERSE,
            2,
            &[(52, 0xffff_ffff), (24, 0x1234_5678), (56, 0x100a), (144, 1)][..],
            4106,
        ),
        (
            FIRST_WRITE_ORDER,
            3,
            &[(52, 0x3333_3333), (24, 0x2222_2222), (56, 0x100f), (144, 2)][..],
            4111,
        ),
        (
            TWO,
            1,
            &[(24, 0x1234_5678), (56, 0x1005), (144, 0)][..],
            4101,
        ),
    ] {
        let block = compile_block_from_bytes(0x1000, bytes, limit).unwrap();
        check(
            flags,
            &block,
            &initial,
            updates,
            next,
            0x1234_5678_9abc_def0,
        );
    }
    for (start, eip, dispatched) in [(0xffff_fffd, 2, 2), (0x7fff_fffd, 0x8000_0002, -2147483646)] {
        let block = compile_block_from_bytes(start, SINGLE_MOVES[0].0, 1).unwrap();
        check(
            flags,
            &block,
            &initial,
            &[(24, 0x1234_5678), (56, eip), (144, 0)],
            dispatched,
            -1,
        );
    }
    let block = compile_block_from_bytes(0x1000, SINGLE_MOVES[0].0, 1).unwrap();
    check(
        flags,
        &block,
        &state(17),
        &[(24, 0x1234_5678), (56, 0x1005), (144, 18)],
        4101,
        i64::MAX,
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn mov_blocks_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn mov_blocks_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}

#[test]
fn register_mov_requires_its_modrm_byte() {
    for opcode in [0x89, 0x8b] {
        assert!(matches!(
            compile_block_from_bytes(0x1000, &[opcode], 1),
            Err(BlockError::TruncatedInstruction {
                address: 0x1000,
                available: 1
            })
        ));
    }
    assert!(matches!(
        compile_block_from_bytes(0xffff_fffe, &[0x89, 0xc1, 0x8b], 2),
        Err(BlockError::TruncatedInstruction {
            address: 0,
            available: 1
        })
    ));
}

#[test]
fn register_copies_forward_values_and_omit_redundant_backing_accesses() {
    fn accesses(bytes: &[u8], instructions: u32) -> (Vec<u64>, Vec<u64>) {
        let module = compile_block_from_bytes(0x1000, bytes, instructions).unwrap();
        Validator::new().validate_all(&module.bytes).unwrap();
        let mut loads = Vec::new();
        let mut stores = Vec::new();
        for payload in Parser::new(0).parse_all(&module.bytes) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                for operation in body.get_operators_reader().unwrap() {
                    match operation.unwrap() {
                        Operator::I32Load { memarg } => loads.push(memarg.offset),
                        Operator::I32Store { memarg } => stores.push(memarg.offset),
                        _ => {}
                    }
                }
            }
        }
        (loads, stores)
    }
    // Both destinations copy the same initial EAX value.
    assert_eq!(
        accesses(&[0x89, 0xc1, 0x8b, 0xd0], 2),
        (vec![24, 144], vec![28, 32, 56, 144])
    );
    assert_eq!(
        accesses(&[0xb8, 42, 0, 0, 0, 0x89, 0xc1, 0x8b, 0xd1, 0x89, 0xd3], 4),
        (vec![144], vec![24, 28, 32, 36, 56, 144])
    );
    // EAX<-EAX and ECX<-ECX retire without changing any register.
    assert_eq!(
        accesses(&[0x89, 0xc0, 0x8b, 0xc9], 2),
        (vec![144], vec![56, 144])
    );
}
