use super::*;
use wasm86_compiler::{VectorLane, I16, I64};

fn masks<T: VectorLane>(v8: bool, input: u128, expected_mask: u32) {
    let mut bytes = vec![0xa5; 24];
    bytes[..16].copy_from_slice(&input.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let vector = body.load::<V128>(memory, 0)?;
        body.store(memory, 16, vector.bitmask::<T>())?;
        body.store(memory, 20, Val::<V128>::from(input).bitmask::<T>())?;
        body.return_(())
    });
    let mut expected = bytes.clone();
    expected[16..20].copy_from_slice(&expected_mask.to_le_bytes());
    expected[20..24].copy_from_slice(&expected_mask.to_le_bytes());
    check(&module, &bytes, &expected, 0, v8);
}

fn widths(v8: bool) {
    masks::<I8>(
        v8,
        0x807f01ff_00fe7e81_aa558000_ff007f80,
        0b10010101_10101001,
    );
    masks::<I16>(v8, 0x80007fff_0001ffff_00ff807f_ff000000, 0b10010110);
    masks::<I32>(v8, 0x80000000_7fffffff_ffffffff_00000080, 0b1010);
    masks::<I64>(v8, 0x7fffffffffffffff_8000000000000000, 0b01);
    masks::<I8>(v8, u128::MAX, 0xffff);
    masks::<I16>(v8, 0, 0);
    let mut fixture = Fixture::new();
    let initial = u128::MAX.to_le_bytes();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let low = body.load::<V128>(memory, 0)?.low_half();
        body.store(memory, 0, low.bitmask::<I8>())?;
        body.store(memory, 4, low.bitmask::<I16>())?;
        body.store(memory, 8, low.bitmask::<I32>())?;
        body.store(memory, 12, low.bitmask::<I64>())?;
        body.return_(())
    });
    let expected: Vec<_> = [0xff_u32, 0xf, 3, 1]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    check(&module, &initial, &expected, 0, v8);
}

#[test]
fn vector_sign_masks_preserve_lane_order_and_clear_upper_bits() {
    widths(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_vector_sign_masks() {
    widths(true);
}
