use super::*;

fn average(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Maximum sums, odd sums, equal operands and zeros.
    check_binary(
        &mut checks,
        0xe0,
        0x102fe010000ffff,
        0xff0200020100feff,
        0x80027f020100ffff,
    );
    check_binary(
        &mut checks,
        0xe3,
        0xffffffff,
        0x10000fffeffff,
        0x10000ffffffff,
    );
}

test_frontends!(mmx_packed_averages, average);
