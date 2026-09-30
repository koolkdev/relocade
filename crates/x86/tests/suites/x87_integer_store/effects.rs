//! Integer-store memory guards, retained source values and sticky status.

use super::*;

fn operand_guards(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let form = FORMS[4];
    let code = form.instruction(0x4ffc);
    let mut case = Case::masked((TOP, 0x7fff), i64::MIN, 0x41);
    case.empty = true;
    case.control = 0x0340;
    let mut image = initial_image(&code, &case);
    image.data(0x8ffc, &[0xa6; 4]);
    for (writable, error) in [(None, 2), (Some(false), 3)] {
        if let Some(writable) = writable {
            image.map(5, 0xe000, writable);
        }
        checks.check(
            "the complete integer write guard precedes stack and conversion exceptions",
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
        "a pending exception precedes the destination access",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );

    let case = Case::masked((TOP, 0xc03e), i64::MIN, 0);
    let mut image = initial_image(&code, &case);
    image.map(5, 0xe000, true);
    image.data(0x8ffc, &[0xa6; 4]);
    image.data(0xe000, &[0xa6; 4]);
    let mut stored = form.completed(image.cpu, 0x4ffc, 6);
    stored.x87.status.top = 0;
    stored.x87.status.c1 = 0;
    stored.x87.tag_word = 0xffff;
    checks.check(
        "a split integer store commits both fragments and the pop",
        &code,
        &image,
        &[Step {
            cpu: stored,
            ram: &[(0x8ffc, &[0; 4]), (0xe000, &[0, 0, 0, 0x80])],
            exit: Exit::Dispatch(stored.eip),
        }],
    );

    for (form, value) in [
        (FORMS[2], (TOP, 0xc00e)),
        (FORMS[3], (TOP, 0xc01e)),
        (FORMS[4], (TOP, 0xc03e)),
    ] {
        let address = 0x5000 - form.bytes as u32;
        let code = [vec![0x66], form.instruction(address)].concat();
        let mut image = initial_image(&code, &Case::masked(value, form.minimum(), 0));
        image.data(address + 0x4000, &[0xa6; 8][..form.bytes]);
        let mut stored = form.completed(image.cpu, address, 7);
        stored.x87.status.top = 0;
        stored.x87.status.c1 = 0;
        stored.x87.tag_word = 0xffff;
        let output = form.minimum().to_le_bytes();
        checks.check(
            "66 preserves the encoded integer store width",
            &code,
            &image,
            &[Step {
                cpu: stored,
                ram: &[(address + 0x4000, &output[..form.bytes])],
                exit: Exit::Dispatch(stored.eip),
            }],
        );
    }
}

fn source_and_sticky_status(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let form = FORMS[1]; // FIST m32 keeps ST0 for the subsequent exact extended store.
    let code = [form.instruction(0x4000), vec![0xdb, 0x3d, 0x20, 0x40, 0, 0]].concat();
    let value = (0xc000_0000_0000_0000, 0xbfff); // -1.5
    let image = initial_image(&code, &Case::masked(value, -2, PE | C1));
    let mut rounded = form.completed(image.cpu, 0x4000, 6);
    rounded.x87.status = status(0x7f20);
    let mut exact = complete_x87(rounded, 6, 0x033d);
    exact.x87.data_offset = 0x4020;
    exact.x87.status = status(0x4520);
    exact.x87.tag_word = 0xffff;
    checks.check(
        "integer rounding retains the exact source and sticky precision",
        &code,
        &image,
        &[
            Step {
                cpu: rounded,
                ram: &[(0x8000, &(-2_i32).to_le_bytes())],
                exit: Exit::Dispatch(rounded.eip),
            },
            Step {
                cpu: exact,
                ram: &[(0x8020, &real80(value))],
                exit: Exit::Dispatch(exact.eip),
            },
        ],
    );

    let form = FORMS[4];
    let code = form.instruction(0x4000);
    let mut case = Case::masked((TOP, 0x7fff), i64::MIN, 1);
    case.control = 0x037e;
    let mut image = initial_image(&code, &case);
    image.cpu.x87.status.invalid = 0x80;
    image.cpu.x87.status.precision = 0x81;
    image.cpu.x87.status.top = 0xaf;
    image.cpu.x87.status.stack_fault = 0x81;
    let mut suppressed = form.completed(image.cpu, 0x4000, 6);
    suppressed.x87.status.invalid = 0x81;
    suppressed.x87.status.error_summary = 1;
    suppressed.x87.status.busy = 1;
    suppressed.x87.status.c1 = 0;
    checks.check(
        "suppressed invalid preserves sticky status, raw TOP and source",
        &code,
        &image,
        &[dispatch(suppressed)],
    );
}

fn load_then_integer_store(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (load, value, source) in [
        (
            [0xdf, 0x2d, 0, 0x40, 0, 0],
            (0xffff_ffff_ffff_fffe, 0x403d),
            i64::MAX.to_le_bytes(),
        ),
        (
            [0xdd, 0x05, 0, 0x40, 0, 0],
            (0xc000_0000_0000_0000, 0x3fff),
            0x3ff8_0000_0000_0000_u64.to_le_bytes(),
        ),
    ] {
        let form = FORMS[4];
        let code = [load.as_slice(), &form.instruction(0x4020)].concat();
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        image.map(4, 0x8000, true);
        image.data(0x8000, &source);
        let integer_source = load[0] == 0xdf;
        let mut loaded = complete_x87(image.cpu, 6, if integer_source { 0x072d } else { 0x0505 });
        loaded.x87.data_offset = 0x4000;
        loaded.x87.data_selector = 0x23;
        loaded.x87.tag_word = 0x3fff;
        loaded.x87.status = status(0x7d00);
        write_register_bits(&mut loaded, 7, value);
        let mut stored = form.completed(loaded, 0x4020, 6);
        stored.x87.tag_word = 0xffff;
        stored.x87.status = status(if integer_source { 0x4500 } else { 0x4720 });
        let output = if integer_source { i64::MAX } else { 2_i64 }.to_le_bytes();
        checks.check(
            "integer stores consume tracked integer and narrow real loads",
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

test_frontends!(access, operand_guards);
test_frontends!(retained_state, source_and_sticky_status);
test_frontends!(loaded, load_then_integer_store);
