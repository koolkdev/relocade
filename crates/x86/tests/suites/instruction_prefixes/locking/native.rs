//! Native atomic encodings and the ordinary-access boundary.

use wasm86_x86::compile_block_from_bytes;
use wasmparser::{Parser, Payload};

fn atomic_instructions(code: &[u8]) -> Vec<String> {
    let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
    let mut atomics = Vec::new();
    for payload in Parser::new(0).parse_all(&module.bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operator in body.get_operators_reader().unwrap() {
                let operator = operator.unwrap();
                let name = format!("{operator:?}");
                let name = name.split_whitespace().next().unwrap();
                if name.contains("Atomic") {
                    atomics.push(name.to_owned());
                }
            }
        }
    }
    atomics
}

#[test]
fn lock_families_use_native_rmw_without_extra_fences() {
    for (code, expected) in [
        (&[0xf0, 0x01, 0x03][..], "I32AtomicRmwAdd"),
        (&[0xf0, 0x11, 0x03], "I32AtomicRmwAdd"),
        (&[0xf0, 0x29, 0x03], "I32AtomicRmwSub"),
        (&[0xf0, 0x19, 0x03], "I32AtomicRmwSub"),
        (&[0xf0, 0x21, 0x03], "I32AtomicRmwAnd"),
        (&[0xf0, 0x09, 0x03], "I32AtomicRmwOr"),
        (&[0xf0, 0x31, 0x03], "I32AtomicRmwXor"),
        (&[0xf0, 0xff, 0x03], "I32AtomicRmwAdd"),
        (&[0xf0, 0xff, 0x0b], "I32AtomicRmwSub"),
        (&[0xf0, 0xf7, 0x13], "I32AtomicRmwXor"),
        (&[0xf0, 0x0f, 0xab, 0x03], "I32AtomicRmwOr"),
        (&[0xf0, 0x0f, 0xb3, 0x03], "I32AtomicRmwAnd"),
        (&[0xf0, 0x0f, 0xbb, 0x03], "I32AtomicRmwXor"),
        (&[0xf0, 0x0f, 0xc1, 0x03], "I32AtomicRmwAdd"),
        (&[0xf0, 0x0f, 0xb1, 0x03], "I32AtomicRmwCmpxchg"),
        (&[0x87, 0x03], "I32AtomicRmwXchg"),
        (&[0xf0, 0x87, 0x03], "I32AtomicRmwXchg"),
        (&[0xf0, 0x00, 0x03], "I32AtomicRmw8AddU"),
        (&[0x66, 0xf0, 0x01, 0x03], "I32AtomicRmw16AddU"),
        (&[0xf0, 0x0f, 0xc7, 0x0b], "I64AtomicRmwCmpxchg"),
    ] {
        assert_eq!(atomic_instructions(code), [expected], "{code:02x?}");
    }
    assert_eq!(
        atomic_instructions(&[0xf0, 0xf7, 0x1b]),
        ["I32AtomicLoad", "I32AtomicRmwCmpxchg"]
    );
}

#[test]
fn ordinary_accesses_and_register_exchanges_do_not_acquire_atomics_or_fences() {
    for code in [
        &[0x8b, 0x03][..],
        &[0x89, 0x03],
        &[0x01, 0x03],
        &[0x11, 0x03],
        &[0xff, 0x03],
        &[0xf7, 0x1b],
        &[0x0f, 0xab, 0x03],
        &[0x0f, 0xc1, 0x03],
        &[0x0f, 0xb1, 0x03],
        &[0x0f, 0xc7, 0x0b],
        &[0x87, 0xc3],
        &[0x86, 0xc4],
    ] {
        assert!(atomic_instructions(code).is_empty(), "{code:02x?}");
    }
}
