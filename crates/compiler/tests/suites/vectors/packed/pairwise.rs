use super::*;

fn pairwise() {
    // Lane pairs differ across the 64-bit boundary, and maximum sums must widen.
    let left = 0xffff000180007fff_0001ff000201ffff_u128;
    let bytes = left.to_le_bytes();
    let byte_sums: Vec<u8> = bytes
        .chunks_exact(2)
        .flat_map(|pair| (u16::from(pair[0]) + u16::from(pair[1])).to_le_bytes())
        .collect();
    let word_sums: Vec<u8> = bytes
        .chunks_exact(4)
        .flat_map(|pair| {
            let a = u16::from_le_bytes([pair[0], pair[1]]);
            let b = u16::from_le_bytes([pair[2], pair[3]]);
            (u32::from(a) + u32::from(b)).to_le_bytes()
        })
        .collect();
    let results = [
        u128::from_le_bytes(byte_sums.try_into().unwrap()),
        u128::from_le_bytes(word_sums.try_into().unwrap()),
    ];
    binary_width_results::<V128, 2>(left, 0, results, |a, _| {
        [
            a.unsigned_pairwise_add_lanes::<I8>(),
            a.unsigned_pairwise_add_lanes::<I16>(),
        ]
    });
    binary_width_results::<V64, 2>(left, 0, results, |a, _| {
        [
            a.unsigned_pairwise_add_lanes::<I8>(),
            a.unsigned_pairwise_add_lanes::<I16>(),
        ]
    });
}

#[test]
fn packed_pairwise_sums_widen_each_adjacent_pair() {
    pairwise();
}
