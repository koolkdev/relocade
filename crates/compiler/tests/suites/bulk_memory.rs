use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{Type, I32};
use wasmparser::{Operator, Parser, Payload};

fn transfers() -> TestModule {
    let mut fixture = Fixture::new();
    fixture.memory("unused", &[]);
    let source = fixture.memory("source", &[1, 2, 3, 4, 5, 6]);
    let destination = fixture.memory("destination", &[0xa5; 12]);
    fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let bytes = body.parameter::<I32>(0)?;
        let before = body.load::<I32>(destination, 2)?;
        body.memory_copy(destination, 2, source, 0, &bytes)?;
        body.memory_copy(destination, 3, destination, 2, &bytes)?;
        body.memory_fill(destination, 9, 0x1234u32, 2)?;
        body.return_(before)
    })
}

fn check_transfers(v8: bool) {
    let module = transfers();
    let expected = &[0xa5, 0xa5, 1, 1, 2, 3, 4, 5, 6, 0x34, 0x34, 0xa5];
    if v8 {
        assert_eq!(
            module.run_v8(&Input::call("run", &[Value::I32(6)]).with_memories(&[
                MemoryBytes::new("destination", &[0xa5; 12]),
                MemoryBytes::new("source", &[1, 2, 3, 4, 5, 6]),
            ])),
            Observation::returned(&[Value::I32(0xa5a5_a5a5u32 as i32)]).with_memories(&[
                MemoryBytes::new("destination", expected),
                MemoryBytes::new("source", &[1, 2, 3, 4, 5, 6]),
            ])
        );
    } else {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((6,)).unwrap(), 0xa5a5_a5a5u32 as i32);
        assert_eq!(&instance.memory("destination")[..12], expected);
        assert_eq!(&instance.memory("source")[..6], &[1, 2, 3, 4, 5, 6]);
    }
}

#[test]
fn bulk_transfers_preserve_snapshots_overlap_and_byte_fill() {
    check_transfers(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_bulk_transfers_preserve_snapshots_overlap_and_byte_fill() {
    check_transfers(true);
}

#[test]
fn bulk_operations_retain_both_memories_and_lower_to_native_wasm() {
    let module = transfers();
    let mut copies = Vec::new();
    let mut fills = Vec::new();
    for payload in Parser::new(0).parse_all(module.bytes()) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operator in body.get_operators_reader().unwrap() {
                match operator.unwrap() {
                    Operator::MemoryCopy { dst_mem, src_mem } => copies.push((dst_mem, src_mem)),
                    Operator::MemoryFill { mem } => fills.push(mem),
                    _ => {}
                }
            }
        }
    }
    assert_eq!(copies, [(1, 0), (1, 1)]);
    assert_eq!(fills, [1]);
}

#[test]
fn calls_summarize_bulk_writes_and_keep_reader_snapshots() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("memory", &[7, 0, 0, 0, 9, 0, 0, 0]);
    let writer = fixture
        .program
        .function(signature(&[], &[]), |mut body| {
            body.memory_copy(memory, 0, memory, 4, 4)?;
            body.return_(())
        })
        .unwrap();
    let reader = fixture
        .program
        .function(signature(&[], &[Type::I32]), |mut body| {
            let value = body.load::<I32>(memory, 0)?;
            body.return_(value)
        })
        .unwrap();
    let module = fixture.function(&[], &[Type::I32], |mut body| {
        let before = body.load::<I32>(memory, 0)?;
        body.call::<()>(writer, &[])?;
        let middle = body.call::<I32>(reader, &[])?;
        body.memory_fill(memory, 0, 0, 4)?;
        body.return_(before.add(middle))
    });
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), 16);
    assert_eq!(&instance.memory("memory")[..8], &[0, 0, 0, 0, 9, 0, 0, 0]);
}

fn check_relative_snapshots(v8: bool) {
    let initial = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    for (offset, order) in [(2, ["load", "fill"]), (4, ["fill", "load"])] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("memory", initial);
        let module = fixture.function(&[Type::I32], &[Type::I32], |mut body| {
            let base = body.parameter::<I32>(0)?;
            let before = body.load_at::<I32>(memory, &base, offset)?;
            body.memory_fill(memory, base, 0, 4)?;
            body.return_(before)
        });
        let mut events = Vec::new();
        for payload in Parser::new(0).parse_all(module.bytes()) {
            if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::I32Load { .. } => events.push("load"),
                        Operator::MemoryFill { .. } => events.push("fill"),
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(events, order);
        for base in [0, 4] {
            let start = base as usize + offset as usize;
            let before = i32::from_le_bytes(initial[start..start + 4].try_into().unwrap());
            let mut expected = initial.to_vec();
            expected[base as usize..base as usize + 4].fill(0);
            if v8 {
                assert_eq!(
                    module.run_v8(
                        &Input::call("run", &[Value::I32(base)])
                            .with_memories(&[MemoryBytes::new("memory", initial)])
                    ),
                    Observation::returned(&[Value::I32(before)])
                        .with_memories(&[MemoryBytes::new("memory", &expected)])
                );
            } else {
                let mut instance = module.instantiate();
                assert_eq!(instance.call::<i32>(base).unwrap(), before);
                assert_eq!(&instance.memory("memory")[..initial.len()], &expected);
            }
        }
    }
}

#[test]
fn relative_bulk_writes_preserve_overlapping_snapshots_and_allow_disjoint_reads() {
    check_relative_snapshots(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_relative_bulk_writes_preserve_snapshots_and_allow_disjoint_reads() {
    check_relative_snapshots(true);
}

#[test]
fn zero_lengths_allow_memory_end_and_out_of_bounds_ranges_do_not_partially_write() {
    for bytes in [0, 3] {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("memory", &[1, 2, 3, 4]);
        let module = fixture.function(&[], &[], |mut body| {
            body.memory_fill(memory, if bytes == 0 { 65536 } else { 65535 }, 9, bytes)?;
            body.return_(())
        });
        let mut instance = module.instantiate();
        let result = instance.call::<()>(());
        assert_eq!(result.is_ok(), bytes == 0);
        assert_eq!(instance.memory("memory")[65535], 0);
    }
}
