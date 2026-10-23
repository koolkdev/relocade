use super::*;
use relocade_compiler::{MemoryType, VectorLane, I16, I64};

fn indexed<T: VectorLane + MemoryType>(width: usize, replacement: u64) {
    let mut bytes = vec![0xa5; 80];
    let original: [u8; 16] = std::array::from_fn(|i| 0x80 + i as u8);
    bytes[..16].copy_from_slice(&original);
    bytes[16..24].copy_from_slice(&replacement.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let index = body.parameter::<I32>(0)?;
        let vector = body.load::<V128>(memory, 0)?;
        let value = body.load::<T>(memory, 16)?;
        body.store(memory, 24, vector.extract_lane::<T>(&index))?;
        body.store(memory, 32, vector.replace_lane(&index, value))?;
        // Refolding literal indices must use the logical width, including I8/I16.
        body.store(
            memory,
            48,
            vector.replace_lane(255, vector.extract_lane::<T>(255)),
        )?;
        body.store(
            memory,
            64,
            vector.replace_lane(255, Val::<T>::from(0xfeedcafe_u32)),
        )?;
        body.return_(())
    });
    for index in [0, 1, 2, 3, 7, 8, 15, 16, 255, -1, i32::MIN] {
        let lane = index as u32 as usize % (16 / width);
        let offset = lane * width;
        let mut expected = bytes.clone();
        expected[24..24 + width].copy_from_slice(&original[offset..offset + width]);
        expected[32..48].copy_from_slice(&original);
        expected[32 + offset..32 + offset + width]
            .copy_from_slice(&replacement.to_le_bytes()[..width]);
        expected[48..64].copy_from_slice(&original);
        expected[64..80].copy_from_slice(&original);
        expected[80 - width..80].copy_from_slice(&0xfeedcafe_u64.to_le_bytes()[..width]);
        check(&module, &expected, index);
    }
}

#[test]
fn vector_runtime_lane_indices_wrap_and_preserve_other_lanes() {
    indexed::<I8>(1, 0x73);
    indexed::<I16>(2, 0xfedc);
    indexed::<I32>(4, 0xfedcba98);
    indexed::<I64>(8, 0xfedcba9876543210);
    narrow();
}

fn narrow() {
    let original: [u8; 16] = std::array::from_fn(|i| 0x80 + i as u8);
    let mut bytes = vec![0xa5; 48];
    bytes[..16].copy_from_slice(&original);
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], move |mut body| {
        let index = body.parameter::<I32>(0)?;
        let vector = body.load::<V128>(memory, 0)?.low_half();
        body.store(memory, 16, vector.extract_lane::<I16>(&index))?;
        body.store(memory, 24, vector.replace_lane::<I16>(&index, 0xabcd))?;
        body.store(memory, 32, vector.replace_lane::<I16>(0, 0xabcd))?;
        body.store(memory, 40, vector.replace_lane::<I16>(7, 0x1234))?;
        body.return_(())
    });
    for index in [0, 1, 2, 3, 4, 7, 8, 255, -1] {
        let offset = (index as usize & 3) * 2;
        let mut expected = bytes.clone();
        expected[16..18].copy_from_slice(&original[offset..offset + 2]);
        for output in [24, 32, 40] {
            expected[output..output + 8].copy_from_slice(&original[..8]);
        }
        expected[24 + offset..26 + offset].copy_from_slice(&0xabcd_u16.to_le_bytes());
        expected[32..34].copy_from_slice(&0xabcd_u16.to_le_bytes());
        expected[46..48].copy_from_slice(&0x1234_u16.to_le_bytes());
        check(&module, &expected, index);
    }
}
