//! Packed-decimal memory transactions and interaction with tracked x87 values.

use super::*;

fn memory_span(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = [vec![0x66], instruction(0x4ff6)].concat();
    let case = Case::masked((0xde0b_6b3a_763f_fff0, 0x403a), MAXIMUM, 0);
    let image = initial_image(&code, &case);
    let mut stored = completed(image.cpu, 7, 0x0735, 0x4ff6);
    stored.x87.status = status(0x4500);
    stored.x87.tag_word = 0xffff;
    checks.check(
        "66 keeps FBSTP's exact ten-byte operand",
        &code,
        &image,
        &[Step {
            cpu: stored,
            ram: &[(0x8ff6, &MAXIMUM)],
            exit: Exit::Dispatch(stored.eip),
        }],
    );

    let code = instruction(0x4ffb);
    let mut image = initial_image(&code, &case);
    image.map(5, 0xc000, true);
    image.data(0x8ffa, &[0xa6; 6]);
    image.data(0xc000, &[0xa6; 6]);
    let mut stored = completed(image.cpu, 6, 0x0735, 0x4ffb);
    stored.x87.status = status(0x4500);
    stored.x87.tag_word = 0xffff;
    checks.check(
        "FBSTP writes both physical fragments and pops",
        &code,
        &image,
        &[Step {
            cpu: stored,
            ram: &[(0x8ffb, &MAXIMUM[..5]), (0xc000, &MAXIMUM[5..])],
            exit: Exit::Dispatch(stored.eip),
        }],
    );

    let code = instruction(0x4ff7);
    let mut case = Case::masked((TOP, 0x7fff), INDEFINITE, 0x41);
    case.empty = true;
    case.control = 0x0340;
    let mut image = initial_image(&code, &case);
    image.data(0x8ff7, &[0xa6; 9]);
    for (writable, error) in [(None, 2), (Some(false), 3)] {
        if let Some(writable) = writable {
            image.map(5, 0xc000, writable);
        }
        checks.check(
            "FBSTP validates its final byte before conversion or stack effects",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x5000,
                    error,
                },
            }],
        );
    }
    image.cpu.x87.status.invalid = 1;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "FBSTP reports a pending exception before accessing its destination",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

fn tracked_loads(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (load, source, value, output, flags) in [
        (
            [0xdf, 0x25, 0, 0x40, 0, 0],
            MAXIMUM.to_vec(),
            (0xde0b_6b3a_763f_fff0, 0x403a),
            MAXIMUM,
            0,
        ),
        (
            [0xdf, 0x2d, 0, 0x40, 0, 0],
            9_007_199_254_740_993_i64.to_le_bytes().to_vec(),
            (0x8000_0000_0000_0400, 0x4034),
            [0x93, 0x09, 0x74, 0x54, 0x92, 0x19, 0x07, 0x90, 0, 0],
            0,
        ),
        (
            [0xdd, 0x05, 0, 0x40, 0, 0],
            0xbff8_0000_0000_0000_u64.to_le_bytes().to_vec(),
            (0xc000_0000_0000_0000, 0xbfff),
            one_digit(2, true),
            PE | C1,
        ),
    ] {
        let code = [load.to_vec(), instruction(0x4020)].concat();
        let mut image = image(&code, 0, 0xffff);
        image.data(0x8000, &source);
        let opcode = (u16::from(load[0] & 7) << 8) | u16::from(load[1]);
        let mut loaded = completed(image.cpu, 6, opcode, 0x4000);
        loaded.x87.status = status(0x7d00);
        loaded.x87.tag_word = 0x3fff;
        write_register_bits(&mut loaded, 7, value);
        let mut stored = completed(loaded, 6, 0x0735, 0x4020);
        stored.x87.status = status(0x4500 | flags);
        stored.x87.tag_word = 0xffff;
        checks.check(
            "FBSTP consumes tracked decimal, integer and binary sources",
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: stored,
                    ram: &[(0x8020, &output)],
                    exit: Exit::Dispatch(stored.eip),
                },
            ],
        );
    }
}

fn sticky_status(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let code = instruction(0x4001);
    let mut case = Case::masked((TOP, 0x7fff), INDEFINITE, 1);
    case.control = 0x037e;
    let mut image = initial_image(&code, &case);
    image.cpu.x87.status.precision = 1;
    let mut suppressed = completed(image.cpu, 6, 0x0735, 0x4001);
    suppressed.x87.status = status(0xfda1);
    checks.check(
        "unmasked decimal invalid preserves sticky precision, source and TOP",
        &code,
        &image,
        &[dispatch(suppressed)],
    );
}

test_frontends!(fbstp_memory, memory_span);
test_frontends!(fbstp_tracked, tracked_loads);
test_frontends!(fbstp_sticky, sticky_status);
