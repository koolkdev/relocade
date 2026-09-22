use crate::support::step;
use step::{Argument, Event, Input, Observation, Outcome, Snapshot, TestModule};
use wasm86_x86::{compile_block_from_bytes, BlockError, CompiledModule, CpuState, Registers};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, ValType, Validator};

const IMMEDIATE: &[u8] = &[0xb8, 0x78, 0x56, 0x34, 0x12];
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
        let mut after_prefix = IMMEDIATE.to_vec();
        after_prefix.extend_from_slice(&instruction[..available]);
        assert!(matches!(
            compile_block_from_bytes(0xffff_fffd, &after_prefix, 2),
            Err(BlockError::TruncatedInstruction { address: 2, available: actual }) if actual == available
        ));
    }
}

#[test]
fn unsupported_selected_opcodes_report_their_instruction_address() {
    for bytes in [&[0xf4][..], &[0xf0, 0xb8, 0, 0, 0, 0][..]] {
        assert!(matches!(
            compile_block_from_bytes(0x1000, bytes, 1),
            Err(BlockError::UnsupportedInstruction { address: 0x1000, opcode }) if opcode == bytes[0]
        ));
    }
    let mut bytes = IMMEDIATE.to_vec();
    bytes.push(0xf4);
    assert!(matches!(
        compile_block_from_bytes(0x1000, &bytes, 2),
        Err(BlockError::UnsupportedInstruction {
            address: 0x1005,
            opcode: 0xf4
        })
    ));
}

#[test]
fn instruction_limit_excludes_valid_partial_and_unsupported_suffixes() {
    let expected = compile_block_from_bytes(0x1000, IMMEDIATE, 1).unwrap();
    for suffix in [&TWO[5..], &[0xbf, 0x12][..], &[0x66][..], &[0xf4][..]] {
        let mut bytes = IMMEDIATE.to_vec();
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

fn abi_state(count: u32) -> CpuState {
    let mut cpu = CpuState::filled(0xa5);
    cpu.segments = wasm86_x86::Segments::flat32();
    cpu.registers = Registers {
        eax: 0x1111_1111,
        ecx: 0x2222_2222,
        edx: 0x3333_3333,
        ebx: 0x4444_4444,
        esp: 0x5555_5555,
        ebp: 0x6666_6666,
        esi: 0x7777_7777,
        edi: 0x8888_8888,
    };
    cpu.eip = 0xdead_beef;
    cpu.instruction_count = count;
    cpu.reserved_tail = 0x1234_5678_u32.to_le_bytes();
    cpu
}

fn check_dispatch_abi(
    block: &CompiledModule,
    initial: &CpuState,
    expected: CpuState,
    dispatched: i32,
    returned: i64,
) {
    let module = TestModule::new(block);
    let snapshot = Snapshot {
        cpu: expected.to_bytes().to_vec(),
        guest: None,
    };
    let input = Input {
        dispatch_return: returned,
        ..Input::new(&initial.to_bytes())
    };
    assert_eq!(
        module.observe(&input, 1),
        Observation {
            events: vec![
                Event::Dispatch {
                    eip: dispatched,
                    snapshot: snapshot.clone()
                },
                Event::Return {
                    outcome: Outcome::Returned(vec![Argument::I64(returned)]),
                    snapshot
                },
            ],
            guest_unchanged: true,
            machine_unchanged: true,
        },
        "entry {}, expected {expected:?}",
        block.entry
    );
}

#[test]
fn cpu_only_block_abi_publishes_before_dispatch_and_returns_the_callback_value() {
    // These cases exercise the CPU-only block interface. Incoming EIP deliberately
    // differs from the compiled origin; the callback sees completed CPU bytes and
    // its full i64 return value passes through unchanged. Ordinary MOV results are
    // covered by the data_moves family.
    for (bytes, limit, start, initial_count, eax, edi, next_eip, count, dispatched, returned) in [
        (
            IMMEDIATE,
            1,
            0x1000,
            u32::MAX,
            0x1234_5678,
            0x8888_8888,
            0x1005,
            0,
            4101,
            i64::MIN,
        ),
        (
            TWO,
            2,
            0x1000,
            u32::MAX,
            0x1234_5678,
            0xffff_ffff,
            0x100a,
            1,
            4106,
            0x1234_5678_9abc_def0,
        ),
        (
            OVERWRITE,
            2,
            0x1000,
            u32::MAX,
            0xffff_ffff,
            0x8888_8888,
            0x100a,
            1,
            4106,
            0x1234_5678_9abc_def0,
        ),
        (
            REVERSE,
            2,
            0x1000,
            u32::MAX,
            0x1234_5678,
            0xffff_ffff,
            0x100a,
            1,
            4106,
            0x1234_5678_9abc_def0,
        ),
        (
            FIRST_WRITE_ORDER,
            3,
            0x1000,
            u32::MAX,
            0x2222_2222,
            0x3333_3333,
            0x100f,
            2,
            4111,
            0x1234_5678_9abc_def0,
        ),
        (
            TWO,
            1,
            0x1000,
            u32::MAX,
            0x1234_5678,
            0x8888_8888,
            0x1005,
            0,
            4101,
            0x1234_5678_9abc_def0,
        ),
        (
            IMMEDIATE,
            1,
            0xffff_fffd,
            u32::MAX,
            0x1234_5678,
            0x8888_8888,
            2,
            0,
            2,
            -1,
        ),
        (
            IMMEDIATE,
            1,
            0x7fff_fffd,
            u32::MAX,
            0x1234_5678,
            0x8888_8888,
            0x8000_0002,
            0,
            -2147483646,
            -1,
        ),
        (
            IMMEDIATE,
            1,
            0x1000,
            17,
            0x1234_5678,
            0x8888_8888,
            0x1005,
            18,
            4101,
            i64::MAX,
        ),
    ] {
        let block = compile_block_from_bytes(start, bytes, limit).unwrap();
        let initial = abi_state(initial_count);
        let mut expected = initial;
        expected.registers.eax = eax;
        expected.registers.edi = edi;
        expected.eip = next_eip;
        expected.instruction_count = count;
        check_dispatch_abi(&block, &initial, expected, dispatched, returned);
    }
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
