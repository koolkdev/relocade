use super::*;
use wasm86_compiler::V64;
use wasmparser::{Operator, Parser, Payload};

fn routing(module: &TestModule) -> (usize, usize) {
    let mut counts = (0, 0);
    for payload in Parser::new(0).parse_all(module.bytes()) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                match op.unwrap() {
                    Operator::I8x16Shuffle { .. } => counts.0 += 1,
                    Operator::I8x16Swizzle => counts.1 += 1,
                    _ => {}
                }
            }
        }
    }
    counts
}

fn narrow(v8: bool) {
    let left: [u8; 16] = std::array::from_fn(|byte| 0x40 + byte as u8);
    let right: [u8; 16] = std::array::from_fn(|byte| 0x80 + byte as u8);
    for group in 0..32_u8 {
        let indices: [u8; 8] = std::array::from_fn(|byte| group * 8 + byte as u8);
        let mut bytes = vec![0xa5; 72];
        bytes[..16].copy_from_slice(&left);
        bytes[16..32].copy_from_slice(&right);
        bytes[32..40].copy_from_slice(&indices);
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &bytes);
        let module = fixture.function(&[Type::I32], &[], |mut body| {
            let a = body.load::<V128>(memory, 0)?.low_half();
            let b = body.load::<V128>(memory, 16)?.low_half();
            let selectors = body.load::<V128>(memory, 32)?.low_half();
            body.store(memory, 48, a.shuffle_bytes(&b, selectors))?;
            body.store(memory, 56, a.shuffle_bytes(&b, indices))?;
            body.store(
                memory,
                64,
                Val::<V64>::from(0x4746454443424140_u64)
                    .shuffle_bytes(0x8786858483828180_u64, indices),
            )?;
            body.return_(())
        });
        assert_eq!(routing(&module).1, 1);
        let result = indices.map(|index| match index {
            0..8 => left[usize::from(index)],
            8..16 => right[usize::from(index - 8)],
            _ => 0,
        });
        let mut expected = bytes.clone();
        for offset in [48, 56, 64] {
            expected[offset..offset + 8].copy_from_slice(&result);
        }
        check(&module, &bytes, &expected, 0, v8);
    }
}

#[test]
fn narrow_shuffles_route_logical_inputs_and_zero_all_invalid_selectors() {
    narrow(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_narrow_shuffle_lowering() {
    narrow(true);
}

fn known_on_path(v8: bool) {
    let mut bytes = vec![0xa5; 48];
    bytes[..16].copy_from_slice(&LEFT.to_le_bytes());
    bytes[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let indices = [0, 16, 31, 32, 255, 15, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21];
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let condition = body.parameter::<I32>(0)?.eq(0);
        let selectors = condition.select(Val::<V128>::from(indices), [255; 16]);
        let left = body.load::<V128>(memory, 0)?;
        let right = body.load::<V128>(memory, 16)?;
        body.if_(condition, |mut arm| {
            arm.store(memory, 32, left.shuffle_bytes(right, selectors))
        })?;
        body.return_(())
    });
    assert_eq!(routing(&module), (1, 0));
    let mut expected = bytes.clone();
    expected[32..].copy_from_slice(&[
        0x10, 0, 0xf0, 0, 0, 1, 0x32, 0xff, 0x54, 0xff, 0x76, 0, 0x98, 0xcc, 0xba, 0xcc,
    ]);
    check(&module, &bytes, &expected, 0, v8);
    check(&module, &bytes, &bytes, 1, v8);
}

#[test]
fn path_known_selectors_use_one_immediate_shuffle_and_preserve_zero_fill() {
    known_on_path(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_path_known_shuffle_selectors() {
    known_on_path(true);
}

#[test]
fn lowered_shuffle_reuses_an_existing_swizzle() {
    let module = Fixture::new().function(&[Type::V128; 3], &[Type::V128; 2], |body| {
        let left = body.parameter::<V128>(0)?;
        let right = body.parameter::<V128>(1)?;
        let indices = body.parameter::<V128>(2)?;
        body.return_((
            left.swizzle_bytes(&indices),
            left.shuffle_bytes(right, indices),
        ))
    });
    assert_eq!(routing(&module), (0, 2));
    let left = wasmtime::V128::from(LEFT);
    let right = wasmtime::V128::from(RIGHT);
    let indices = wasmtime::V128::from(u128::from_le_bytes([
        0, 16, 15, 31, 255, 32, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21,
    ]));
    let actual: (wasmtime::V128, wasmtime::V128) =
        module.instantiate().call((left, right, indices)).unwrap();
    assert_eq!(
        actual.0.as_u128(),
        u128::from_le_bytes([0x10, 0, 1, 0, 0, 0, 0x32, 0, 0x54, 0, 0x76, 0, 0x98, 0, 0xba, 0])
    );
    assert_eq!(
        actual.1.as_u128(),
        u128::from_le_bytes([
            0x10, 0, 1, 0xf0, 0, 0, 0x32, 0xff, 0x54, 0xff, 0x76, 0, 0x98, 0xcc, 0xba, 0xcc
        ])
    );
}
