use super::*;

fn minmax(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    // PMINUB: opposite signs, equality and both operand choices.
    check_binary(
        &mut checks,
        0xda,
        0x27b007b7f80ff00027b007b7f80ff00,
        0x37aff7b807f0001037aff7b807f0001,
        0x27a007b7f7f0000027a007b7f7f0000,
    );
    // PMAXUB: opposite signs, equality and both operand choices.
    check_binary(
        &mut checks,
        0xde,
        0x27b007b7f80ff00027b007b7f80ff00,
        0x37aff7b807f0001037aff7b807f0001,
        0x37bff7b8080ff01037bff7b8080ff01,
    );
    // PMINSW: opposite signs, equality and both operand choices.
    check_binary(
        &mut checks,
        0xea,
        0x2007b0000007b7fff8000ffff0000,
        0x3007affff007b80007fff00000001,
        0x2007affff007b80008000ffff0000,
    );
    // PMAXSW: opposite signs, equality and both operand choices.
    check_binary(
        &mut checks,
        0xee,
        0x2007b0000007b7fff8000ffff0000,
        0x3007affff007b80007fff00000001,
        0x3007b0000007b7fff7fff00000001,
    );
}

test_frontends!(sse_packed_minmax, minmax);
