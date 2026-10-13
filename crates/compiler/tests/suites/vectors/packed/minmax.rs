use super::*;
use wasm86_compiler::AtLeast;

fn lanes<T: VectorLane>(v8: bool, left: u128, right: u128, results: [u128; 4])
where
    I32: AtLeast<T>,
{
    binary_width_results::<V128, 4>(v8, left, right, results, |a, b| {
        [
            a.signed_min_lanes::<T>(&b),
            a.unsigned_min_lanes::<T>(&b),
            a.signed_max_lanes::<T>(&b),
            a.unsigned_max_lanes::<T>(&b),
        ]
    });
    binary_width_results::<V64, 4>(v8, left, right, results, |a, b| {
        [
            a.signed_min_lanes::<T>(&b),
            a.unsigned_min_lanes::<T>(&b),
            a.signed_max_lanes::<T>(&b),
            a.unsigned_max_lanes::<T>(&b),
        ]
    });
}

fn minmax(v8: bool) {
    // Opposite signs, equal lanes and signed extrema distinguish all four operations.
    lanes::<I8>(
        v8,
        0x7f80ff017f80ff017f80ff017f80ff01,
        0x807f0001807f0001807f0001807f0001,
        [
            0x8080ff018080ff018080ff018080ff01,
            0x7f7f00017f7f00017f7f00017f7f0001,
            0x7f7f00017f7f00017f7f00017f7f0001,
            0x8080ff018080ff018080ff018080ff01,
        ],
    );
    lanes::<I16>(
        v8,
        0x7fff8000ffff00017fff8000ffff0001,
        0x80007fff0000000180007fff00000001,
        [
            0x80008000ffff000180008000ffff0001,
            0x7fff7fff000000017fff7fff00000001,
            0x7fff7fff000000017fff7fff00000001,
            0x80008000ffff000180008000ffff0001,
        ],
    );
    lanes::<I32>(
        v8,
        0x7fffffff80000000ffffffff00000001,
        0x800000007fffffff0000000000000001,
        [
            0x8000000080000000ffffffff00000001,
            0x7fffffff7fffffff0000000000000001,
            0x7fffffff7fffffff0000000000000001,
            0x8000000080000000ffffffff00000001,
        ],
    );
}

#[test]
fn packed_minmax_signedness_and_widths() {
    minmax(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_packed_minmax_signedness_and_widths() {
    minmax(true);
}
