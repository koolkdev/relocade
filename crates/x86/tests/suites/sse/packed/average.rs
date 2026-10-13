use super::*;

fn average(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    // Maximum sums, odd sums, equal operands and zeros.
    check_binary(
        &mut checks,
        0xe0,
        0x102fe010000ffff0102fe010000ffff,
        0xff0200020100feffff0200020100feff,
        0x80027f020100ffff80027f020100ffff,
    );
    check_binary(
        &mut checks,
        0xe3,
        0x10002fffe000100000000ffffffff,
        0xffff00020000000200010000fffeffff,
        0x800000027fff000200010000ffffffff,
    );
}

test_frontends!(sse_packed_averages, average);
