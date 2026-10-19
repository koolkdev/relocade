use super::*;
use wasm86_compiler::AtLeast;

fn lanes<T: VectorLane>(left: u128, right: u128, result: u128)
where
    I16: AtLeast<T>,
{
    binary_width_results::<V128, 1>(left, right, [result], |a, b| {
        [a.unsigned_rounded_average_lanes::<T>(b)]
    });
    binary_width_results::<V64, 1>(left, right, [result], |a, b| {
        [a.unsigned_rounded_average_lanes::<T>(b)]
    });
}

#[test]
fn packed_averages_round_up_without_overflow() {
    lanes::<I8>(
        0x0102fe010000ffff0102fe010000ffff,
        0xff0200020100feffff0200020100feff,
        0x80027f020100ffff80027f020100ffff,
    );
    lanes::<I16>(
        0x00010002fffe000100000000ffffffff,
        0xffff00020000000200010000fffeffff,
        0x800000027fff000200010000ffffffff,
    );
}
