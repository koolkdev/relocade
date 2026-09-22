//! Binary80 encodings and complete ten-byte memory commitments.

use super::*;

fn raw_extended_roundtrips(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [
        0x66, 0xdb, 0x2d, 0, 0x40, 0, 0, 0xdb, 0x3d, 0x20, 0x40, 0, 0,
    ];
    for (name, significand, sign_exponent, loaded_tags) in [
        ("full significand", 0x8000_0000_0000_0001, 0x3fff, 0xffcf),
        ("positive zero", 0, 0, 0xffdf),
        ("negative zero", 0, 0x8000, 0xffdf),
        ("true subnormal", 1, 0, 0xffef),
        ("pseudo-denormal", 0x8000_0000_0000_0007, 0, 0xffef),
        ("pseudo-denormal power", 0x8000_0000_0000_0000, 0, 0xffef),
        ("positive infinity", 0x8000_0000_0000_0000, 0x7fff, 0xffef),
        ("negative infinity", 0x8000_0000_0000_0000, 0xffff, 0xffef),
        ("signaling NaN", 0x8000_0000_0000_0123, 0x7fff, 0xffef),
        ("negative quiet NaN", 0xc000_0000_0000_0456, 0xffff, 0xffef),
        (
            "unsupported unnormal",
            0x0123_4567_89ab_cdef,
            0x4000,
            0xffef,
        ),
        ("unsupported pseudo-infinity", 0, 0x7fff, 0xffef),
    ] {
        let mut image = super::initial_image(&code, 3, 0xffff);
        // PC24 and unmasked invalid/denormal exceptions do not change raw80
        // movement. No arithmetic conversion or operand exception is permitted.
        set_control(&mut image.cpu.x87.control, 0x007c);
        image.map(4, 0x8000, true);
        let bytes = real80((significand, sign_exponent));
        image.data(0x8000, &bytes);
        image.data(0x801f, &[0xa6; 12]);
        let mut loaded = complete(image.cpu, 7, 0x032d);
        loaded.x87.status = status(0x5520);
        loaded.x87.tag_word = loaded_tags;
        loaded.x87.data_offset = 0x4000;
        loaded.x87.data_selector = 0x23;
        write_register_bits(&mut loaded, 2, (significand, sign_exponent));
        let mut stored = complete(loaded, 6, 0x033d);
        stored.x87.status = status(0x5d20);
        stored.x87.tag_word = 0xffff;
        stored.x87.data_offset = 0x4020;
        checks.check(
            name,
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: stored,
                    ram: &[(0x8020, &bytes)],
                    exit: Exit::Dispatch(stored.eip),
                },
            ],
        );
    }
}

fn memory_stack_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for masked in [true, false] {
        let code = [0xdb, 0x3d, 1, 0x40, 0, 0];
        let mut image = super::initial_image(&code, 0, 3);
        set_control(
            &mut image.cpu.x87.control,
            if masked { 0x037f } else { 0x037e },
        );
        image.map(4, 0x8000, true);
        image.data(0x8000, &[0x6a; 12]);
        let mut cpu = complete(image.cpu, 6, 0x033d);
        cpu.x87.data_offset = 0x4001;
        cpu.x87.data_selector = 0x23;
        cpu.x87.status = status(if masked { 0x4d61 } else { 0xc5e1 });
        let indefinite = real80(INDEFINITE);
        let writes = [(0x8001, indefinite.as_slice())];
        checks.check(
            if masked {
                "masked FSTP m80 underflow stores indefinite and pops"
            } else {
                "unmasked FSTP m80 underflow suppresses store and pop"
            },
            &code,
            &image,
            &[Step {
                cpu,
                ram: if masked { &writes } else { &[] },
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }

    let code = [0xdb, 0x2d, 1, 0x40, 0, 0];
    let mut image = super::initial_image(&code, 0, 0);
    image.map(4, 0x8000, false);
    image.data(0x8001, &real80((0x8000_0000_0000_0000, 0x3fff)));
    let mut cpu = complete(image.cpu, 6, 0x032d);
    cpu.x87.status = status(0x7f61);
    cpu.x87.tag_word = 0x8000;
    cpu.x87.data_offset = 0x4001;
    cpu.x87.data_selector = 0x23;
    write_register_bits(&mut cpu, 7, INDEFINITE);
    checks.check(
        "masked FLD m80 overflow pushes indefinite",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

fn ten_byte_memory_boundaries(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let bits = (0xdead_beef_1234_5678, 0x4567);
    let bytes = real80(bits);
    let code = [0xdb, 0x2d, 0xf9, 0x4f, 0, 0, 0xdb, 0x3d, 0xf9, 0x6f, 0, 0];
    let mut image = super::initial_image(&code, 0, 0xffff);
    image.map(4, 0x8000, false);
    image.map(5, 0xa000, false);
    image.map(6, 0xc000, true);
    image.map(7, 0xe000, true);
    image.data(0x8ff9, &bytes[..7]);
    image.data(0xa000, &bytes[7..]);
    image.data(0xcff8, &[0x4a; 8]);
    image.data(0xe000, &[0x4a; 4]);
    let mut loaded = complete(image.cpu, 6, 0x032d);
    loaded.x87.status = status(0x7d20);
    loaded.x87.tag_word = 0x3fff;
    loaded.x87.data_offset = 0x4ff9;
    loaded.x87.data_selector = 0x23;
    write_register_bits(&mut loaded, 7, bits);
    let mut stored = complete(loaded, 6, 0x033d);
    stored.x87.status = status(0x4520);
    stored.x87.tag_word = 0xffff;
    stored.x87.data_offset = 0x6ff9;
    checks.check(
        "raw80 transfers cross scattered pages without touching an eleventh byte",
        &code,
        &image,
        &[
            dispatch(loaded),
            Step {
                cpu: stored,
                ram: &[(0xcff9, &bytes[..7]), (0xe000, &bytes[7..])],
                exit: Exit::Dispatch(stored.eip),
            },
        ],
    );

    let code = [0xdb, 0x2d, 0xf6, 0x4f, 0, 0];
    let mut image = super::initial_image(&code, 0, 0xffff);
    image.map(4, 0xf000, false);
    image.data(0xfff6, &bytes);
    let mut cpu = complete(image.cpu, 6, 0x032d);
    cpu.x87.status = status(0x7d20);
    cpu.x87.tag_word = 0x3fff;
    cpu.x87.data_offset = 0x4ff6;
    cpu.x87.data_selector = 0x23;
    write_register_bits(&mut cpu, 7, bits);
    checks.check(
        "m80 ending at backing boundary reads exactly ten bytes",
        &code,
        &image,
        &[dispatch(cpu)],
    );
}

fn memory_fault_commitment(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (name, modrm, error, tags, control) in [
        (
            "FLD m80 missing final page suppresses push",
            0x2d,
            0,
            0xffff,
            0x037f,
        ),
        (
            "FSTP m80 missing final page suppresses store and pop",
            0x3d,
            2,
            0,
            0x037f,
        ),
        (
            "FLD m80 page fault precedes unmasked stack overflow",
            0x2d,
            0,
            0,
            0x037e,
        ),
        (
            "FSTP m80 page fault precedes unmasked stack underflow",
            0x3d,
            2,
            3,
            0x037e,
        ),
    ] {
        let code = [0xdb, modrm, 0xf9, 0x4f, 0, 0];
        let mut image = super::initial_image(&code, 0, tags);
        set_control(&mut image.cpu.x87.control, control);
        image.map(4, 0x8000, true);
        image.data(0x8ff9, &[0x71; 7]);
        checks.check(
            name,
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
    let code = [0xdb, 0x3d, 0xf9, 0x4f, 0, 0];
    let mut image = super::initial_image(&code, 0, 0);
    image.map(4, 0x8000, true);
    image.map(5, 0xa000, false);
    image.data(0x8ff9, &[0x71; 7]);
    image.data(0xa000, &[0x72; 3]);
    checks.check(
        "FSTP m80 read-only final page prevents a partial first-page write",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x5000,
                error: 3,
            },
        }],
    );

    let code = [0xd9, 0xc0, 0xdb, 0x3d, 0, 0x40, 0, 0];
    let image = super::initial_image(&code, 0, 0xc000);
    let mut pushed = complete(image.cpu, 2, 0x01c0);
    pushed.x87.status = status(0x7d20);
    pushed.x87.tag_word = 0;
    write_register_bits(&mut pushed, 7, register_bits(&image.cpu, 0));
    checks.check(
        "later store fault publishes an earlier completed push",
        &code,
        &image,
        &[
            dispatch(pushed),
            Step {
                cpu: pushed,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x4000,
                    error: 2,
                },
            },
        ],
    );

    let code = [0xdb, 0x3d, 0, 0x40, 0, 0];
    let mut image = super::initial_image(&code, 3, 0);
    image.cpu.segments.ds.limit = 0x4008;
    image.map(4, 0x8000, true);
    image.data(0x8000, &[0x6b; 10]);
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Segmented32);
    checks.check(
        "FSTP m80 final byte beyond the segment prevents the whole store and pop",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        }],
    );
}

test_frontends!(raw_extended, raw_extended_roundtrips);
test_frontends!(memory_stack, memory_stack_faults);
test_frontends!(ten_byte_transfers, ten_byte_memory_boundaries);
test_frontends!(memory_faults, memory_fault_commitment);
