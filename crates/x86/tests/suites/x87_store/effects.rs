//! Store completion, memory faults and source preservation.

use super::*;

fn memory_guards(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        let address = 0x5001 - format.bytes as u32;
        let code = format.instruction(address, true);
        let mut case = StoreCase::masked((TOP | 1, 0x7fff), 0, 0);
        case.empty = true;
        case.control = 0x037e;
        let mut image = initial_image(&code, &case);
        image.data(address + 0x4000, &[0xa6; 8][..format.bytes - 1]);
        for (writable, error) in [(None, 2), (Some(false), 3)] {
            if let Some(writable) = writable {
                image.map(5, 0xe000, writable);
            }
            checks.check(
                "complete write guard precedes stack and conversion exceptions",
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
        image.cpu.x87.status.error_summary = 1;
        image.cpu.x87.status.busy = 1;
        image.cpu.x87.status.invalid = 1;
        checks.check(
            "pending exception precedes operand access",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::FloatingPoint,
            }],
        );

        // A successful noncontiguous split writes every byte, then pops once.
        let case = StoreCase::masked((TOP, 0x3fff), format.one, 0);
        let mut image = initial_image(&code, &case);
        image.map(5, 0xe000, true);
        let mut stored = format.completed(image.cpu, true);
        stored.x87.data_offset = address;
        stored.x87.status.c1 = 0;
        stored.x87.status.top = 0;
        stored.x87.tag_word = 0xffff;
        let output = format.one.to_le_bytes();
        checks.check(
            "split narrow store",
            &code,
            &image,
            &[Step {
                cpu: stored,
                ram: &[
                    (address + 0x4000, &output[..format.bytes - 1]),
                    (0xe000, &output[format.bytes - 1..format.bytes]),
                ],
                exit: Exit::Dispatch(stored.eip),
            }],
        );

        let address = 0x5000 - format.bytes as u32;
        let code = [vec![0x66], format.instruction(address, true)].concat();
        let image = initial_image(&code, &case);
        let mut stored = format.completed(image.cpu, true);
        stored.eip += 1;
        stored.x87.data_offset = address;
        stored.x87.status.c1 = 0;
        stored.x87.status.top = 0;
        stored.x87.tag_word = 0xffff;
        checks.check(
            "operand-size prefix preserves store width at page boundary",
            &code,
            &image,
            &[Step {
                cpu: stored,
                ram: &[(address + 0x4000, &output[..format.bytes])],
                exit: Exit::Dispatch(stored.eip),
            }],
        );
    }
}

fn cleared_store_can_retry(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        let code = [
            format.instruction(0x4000, true),
            vec![0xdb, 0xe2],
            format.instruction(0x4000, false),
        ]
        .concat();
        let mut case = StoreCase::masked((TOP | 1, 0x7fff), 0, 1 | PENDING);
        case.control = 0x037e;
        let image = initial_image(&code, &case);
        let mut suppressed = format.completed(image.cpu, true);
        suppressed.x87.status = status(0xfd81);
        let mut cleared = suppressed;
        cleared.eip += 2;
        cleared.instruction_count = cleared.instruction_count.wrapping_add(1);
        cleared.x87.status = status(0x7d00);
        let mut retried = format.completed(cleared, false);
        retried.x87.status = suppressed.x87.status;
        checks.check(
            "FNCLEX retains a suppressed pop for the next store",
            &code,
            &image,
            &[dispatch(suppressed), dispatch(cleared), dispatch(retried)],
        );
    }
}

fn stores_preserve_loaded_value_and_sticky_precision(engine: Engine, frontend: Frontend) {
    let code = [
        0xdd, 0x05, 0x00, 0x40, 0, 0, // FLD m64
        0xd9, 0x15, 0x20, 0x40, 0, 0, // FST m32
        0xdd, 0x1d, 0x28, 0x40, 0, 0, // FSTP m64
    ];
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.status.precision = 0;
    image.map(4, 0x8000, true);
    let original = 0x3ff0_0000_1000_0001_u64.to_le_bytes();
    image.data(0x8000, &original);
    image.data(0x801f, &[0xa6; 18]);
    let mut loaded = complete_x87(image.cpu, 6, 0x0505);
    loaded.x87.status = status(0x7d00);
    loaded.x87.tag_word = 0x3fff;
    loaded.x87.data_offset = 0x4000;
    loaded.x87.data_selector = 0x23;
    write_register_bits(&mut loaded, 7, (0x8000_0080_0000_0800, 0x3fff));
    let mut rounded = complete_x87(loaded, 6, 0x0115);
    rounded.x87.status = status(0x7f20);
    rounded.x87.data_offset = 0x4020;
    let mut exact = complete_x87(rounded, 6, 0x051d);
    exact.x87.status = status(0x4520);
    exact.x87.tag_word = 0xffff;
    exact.x87.data_offset = 0x4028;
    checks.check(
        "narrow store retains the full source; exact store clears C1 and retains PE",
        &code,
        &image,
        &[
            dispatch(loaded),
            Step {
                cpu: rounded,
                ram: &[(0x8020, &0x3f80_0001_u32.to_le_bytes())],
                exit: Exit::Dispatch(rounded.eip),
            },
            Step {
                cpu: exact,
                ram: &[(0x8028, &original)],
                exit: Exit::Dispatch(exact.eip),
            },
        ],
    );
}

fn suppressed_range_preserves_sticky_precision(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        let code = format.instruction(0x4000, true);
        let mut case = StoreCase::masked((TOP, format.maximum_exponent + 1), 0, 0);
        case.control = 0x0377;
        let mut image = initial_image(&code, &case);
        image.cpu.x87.status.precision = 0x81;
        image.cpu.x87.status.overflow = 0x80;
        image.cpu.x87.status.stack_fault = 1;
        image.cpu.x87.control.rounding_control |= 0x80;
        let mut stored = format.completed(image.cpu, true);
        stored.x87.status.overflow = 0x81;
        stored.x87.status.error_summary = 1;
        stored.x87.status.busy = 1;
        stored.x87.status.c1 = 0;
        checks.check(
            "suppressed range store preserves earlier PE and SF",
            &code,
            &image,
            &[dispatch(stored)],
        );
    }
}

test_frontends!(access, memory_guards);
test_frontends!(retry, cleared_store_can_retry);
test_frontends!(
    loaded_source,
    stores_preserve_loaded_value_and_sticky_precision
);
test_frontends!(sticky_status, suppressed_range_preserves_sticky_precision);
