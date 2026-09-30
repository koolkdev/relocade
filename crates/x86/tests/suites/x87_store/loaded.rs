//! Exact loaded values keep their identity and exception evidence through stores.

use super::*;
use crate::support::x87::INDEFINITE;

fn load(format: Format, address: u32) -> Vec<u8> {
    [&[format.opcode, 0x05], address.to_le_bytes().as_slice()].concat()
}

fn loaded(mut cpu: CpuState, format: Format, bits: (u64, u16), tag: u16) -> CpuState {
    cpu = complete_x87(cpu, 6, (u16::from(format.opcode & 7) << 8) | 5);
    cpu.x87.status.top = cpu.x87.status.top.wrapping_sub(1) & 7;
    cpu.x87.status.c1 = 0;
    let slot = usize::from(cpu.x87.status.top);
    cpu.x87.tag_word = (cpu.x87.tag_word & !(3 << (slot * 2))) | (tag << (slot * 2));
    cpu.x87.data_offset = 0x4000;
    cpu.x87.data_selector = 0x23;
    write_register_bits(&mut cpu, slot, bits);
    cpu
}

fn same_width_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        let code = [
            load(format, 0x4000),
            format.instruction(0x4020, true),
            vec![0x9b],
        ]
        .concat();
        // Expected extended encodings describe exact powers, endpoints and NaN
        // payload positions independently of the generated conversion algorithm.
        for (input, extended, tag, flags) in [
            (0, (0, 0), 1, 0),
            (format.sign(), (0, 0x8000), 1, 0),
            (format.one + 1, (TOP + format.unit(), 0x3fff), 0, 0),
            (format.sign() | format.one, (TOP, 0xbfff), 0, 0),
            (
                format.infinity - 1,
                (0_u64.wrapping_sub(format.unit()), format.maximum_exponent),
                0,
                0,
            ),
            (
                1,
                (TOP, format.minimum_exponent - format.fraction_bits as u16),
                0,
                2,
            ),
            (
                format.sign() | (format.minimum_normal() - 1),
                (
                    0_u64.wrapping_sub(2 * format.unit()),
                    0x8000 | (format.minimum_exponent - 1),
                ),
                0,
                2,
            ),
            (format.infinity, (TOP, 0x7fff), 2, 0),
            (format.sign() | format.infinity, (TOP, 0xffff), 2, 0),
            (
                format.infinity | format.quiet_bit() | 1,
                (0xc000_0000_0000_0000 | format.unit(), 0x7fff),
                2,
                0,
            ),
            (
                format.sign() | format.infinity | 1,
                (0xc000_0000_0000_0000 | format.unit(), 0xffff),
                2,
                1,
            ),
        ] {
            // One normal value exercises RC independence; the class boundaries
            // exercise identity, and only subnormals vary the underflow mask.
            let rounding_modes = if input == format.one + 1 { 4 } else { 1 };
            let underflow_masks: &[bool] = if flags == 2 { &[false, true] } else { &[false] };
            for rc in 0..rounding_modes {
                for &unmasked_underflow in underflow_masks {
                    let mut image = stack_image(&code, 0, 0xffff);
                    image.cpu.x87.status.precision = 0x80;
                    set_control(
                        &mut image.cpu.x87.control,
                        (0x037f | (rc << 10)) & if unmasked_underflow { !0x10 } else { 0xffff },
                    );
                    image.map(4, 0x8000, true);
                    image.data(0x8000, &input.to_le_bytes()[..format.bytes]);
                    image.data(0x801f, &[0xa6; 10]);
                    let mut pushed = loaded(image.cpu, format, extended, tag);
                    pushed.x87.status.invalid = flags & 1;
                    pushed.x87.status.denormal = (flags >> 1) & 1;
                    let mut stored = format.completed(pushed, true);
                    stored.x87.data_offset = 0x4020;
                    let suppressed = flags == 2 && unmasked_underflow;
                    if suppressed {
                        stored.x87.status.underflow = 1;
                        stored.x87.status.error_summary = 1;
                        stored.x87.status.busy = 1;
                    } else {
                        stored.x87.status.top = 0;
                        stored.x87.tag_word = 0xffff;
                    }
                    let output =
                        (input | if flags == 1 { format.quiet_bit() } else { 0 }).to_le_bytes();
                    let writes = if suppressed {
                        vec![]
                    } else {
                        vec![(0x8020, &output[..format.bytes])]
                    };
                    let mut waited = stored;
                    let exit = if suppressed {
                        Exit::FloatingPoint
                    } else {
                        waited.eip += 1;
                        waited.instruction_count = waited.instruction_count.wrapping_add(1);
                        Exit::Dispatch(waited.eip)
                    };
                    checks.check(
                        &format!("exact {}-bit store {input:x}, RC {rc}, unmasked UM {unmasked_underflow}", format.bytes * 8),
                        &code,
                        &image,
                        &[dispatch(pushed), Step { cpu: stored, ram: &writes, exit: Exit::Dispatch(stored.eip) }, Step { cpu: waited, ram: &[], exit }],
                    );
                }
            }
        }
    }
}

fn register_movement(engine: Engine, frontend: Frontend) {
    let [single, double] = FORMATS;
    let code = [
        load(single, 0x4000),
        load(double, 0x4008),
        vec![0xd9, 0xc9], // FXCH ST1
        vec![0xdd, 0xd2], // FST ST2
        single.instruction(0x4020, true),
        double.instruction(0x4028, true),
        single.instruction(0x4030, true),
    ]
    .concat();
    let mut image = stack_image(&code, 0, 0xffff);
    image.map(4, 0x8000, true);
    image.data(0x8000, &0x3f80_0001_u32.to_le_bytes());
    image.data(0x8008, &0xc000_0000_0000_0001_u64.to_le_bytes());
    let single_bits = (0x8000_0100_0000_0000, 0x3fff);
    let double_bits = (0x8000_0000_0000_0800, 0xc000);
    let first = loaded(image.cpu, single, single_bits, 0);
    let mut second = loaded(first, double, double_bits, 0);
    second.x87.data_offset = 0x4008;
    let mut exchanged = complete_x87(second, 2, 0x01c9);
    write_register_bits(&mut exchanged, 6, single_bits);
    write_register_bits(&mut exchanged, 7, double_bits);
    let mut copied = complete_x87(exchanged, 2, 0x05d2);
    copied.x87.tag_word &= !3;
    write_register_bits(&mut copied, 0, single_bits);
    let mut stored_a = single.completed(copied, true);
    stored_a.x87.data_offset = 0x4020;
    stored_a.x87.status.top = 7;
    stored_a.x87.tag_word |= 3 << 12;
    let mut stored_b = double.completed(stored_a, true);
    stored_b.x87.data_offset = 0x4028;
    stored_b.x87.status.top = 0;
    stored_b.x87.tag_word |= 3 << 14;
    let mut stored_copy = single.completed(stored_b, true);
    stored_copy.x87.data_offset = 0x4030;
    stored_copy.x87.status.top = 1;
    stored_copy.x87.tag_word = 0xffff;
    ImageSequences::new(engine, frontend, SegmentProfile::Flat32).check(
        "mixed-width loaded values survive exchange, register copy and pop",
        &code,
        &image,
        &[
            dispatch(first),
            dispatch(second),
            dispatch(exchanged),
            dispatch(copied),
            Step {
                cpu: stored_a,
                ram: &[(0x8020, &0x3f80_0001_u32.to_le_bytes())],
                exit: Exit::Dispatch(stored_a.eip),
            },
            Step {
                cpu: stored_b,
                ram: &[(0x8028, &0xc000_0000_0000_0001_u64.to_le_bytes())],
                exit: Exit::Dispatch(stored_b.eip),
            },
            Step {
                cpu: stored_copy,
                ram: &[(0x8030, &0x3f80_0001_u32.to_le_bytes())],
                exit: Exit::Dispatch(stored_copy.eip),
            },
        ],
    );
}

fn loaded_values_at_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        let code = [load(format, 0x4000), format.instruction(0x5000, true)].concat();
        let mut image = stack_image(&code, 0, 0xffff);
        image.map(4, 0x8000, true);
        image.data(0x8000, &format.one.to_le_bytes()[..format.bytes]);
        let pushed = loaded(image.cpu, format, (TOP, 0x3fff), 0);
        checks.check(
            "destination fault publishes the loaded extended payload without popping",
            &code,
            &image,
            &[
                dispatch(pushed),
                Step {
                    cpu: pushed,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x5000,
                        error: 2,
                    },
                },
            ],
        );

        let code = [
            load(format, 0x4000),
            vec![0xdd, 0xc0],
            format.instruction(0x4020, true),
        ]
        .concat();
        let mut image = stack_image(&code, 0, 0xffff);
        image.map(4, 0x8000, true);
        image.data(0x8000, &format.one.to_le_bytes()[..format.bytes]);
        let pushed = loaded(image.cpu, format, (TOP, 0x3fff), 0);
        let mut freed = complete_x87(pushed, 2, 0x05c0);
        freed.x87.tag_word = 0xffff;
        let mut stored = format.completed(freed, true);
        stored.x87.data_offset = 0x4020;
        stored.x87.status.top = 0;
        stored.x87.status.invalid = 1;
        stored.x87.status.stack_fault = 1;
        let indefinite = (format.sign() | format.infinity | format.quiet_bit()).to_le_bytes();
        checks.check(
            "freeing a loaded value makes the store use indefinite and retain the payload",
            &code,
            &image,
            &[
                dispatch(pushed),
                dispatch(freed),
                Step {
                    cpu: stored,
                    ram: &[(0x8020, &indefinite[..format.bytes])],
                    exit: Exit::Dispatch(stored.eip),
                },
            ],
        );

        // A masked stack overflow replaces the loaded value, even though its
        // original narrow bits remain available during block construction.
        let code = [load(format, 0x4000), format.instruction(0x4020, true)].concat();
        let mut image = stack_image(&code, 0, 0);
        image.map(4, 0x8000, true);
        image.data(0x8000, &format.one.to_le_bytes()[..format.bytes]);
        let mut pushed = loaded(image.cpu, format, INDEFINITE, 2);
        pushed.x87.status.invalid = 1;
        pushed.x87.status.stack_fault = 1;
        pushed.x87.status.c1 = 1;
        let mut stored = format.completed(pushed, true);
        stored.x87.data_offset = 0x4020;
        stored.x87.status.c1 = 0;
        stored.x87.status.top = 0;
        stored.x87.tag_word = 0xc000;
        let indefinite = (format.sign() | format.infinity | format.quiet_bit()).to_le_bytes();
        checks.check(
            "stack overflow substitution survives a same-width store",
            &code,
            &image,
            &[
                dispatch(pushed),
                Step {
                    cpu: stored,
                    ram: &[(0x8020, &indefinite[..format.bytes])],
                    exit: Exit::Dispatch(stored.eip),
                },
            ],
        );
    }
}

fn cleared_suppressed_load(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        let code = [
            load(format, 0x4000),
            load(format, 0x4008),
            vec![0xdb, 0xe2],
            format.instruction(0x4020, true),
        ]
        .concat();
        let mut image = stack_image(&code, 0, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.map(4, 0x8000, true);
        image.data(0x8000, &format.one.to_le_bytes()[..format.bytes]);
        image.data(0x8008, &(format.infinity | 1).to_le_bytes()[..format.bytes]);
        let pushed = loaded(image.cpu, format, (TOP, 0x3fff), 0);
        let mut suppressed = complete_x87(pushed, 6, (u16::from(format.opcode & 7) << 8) | 5);
        suppressed.x87.data_offset = 0x4008;
        suppressed.x87.status.invalid = 1;
        suppressed.x87.status.error_summary = 1;
        suppressed.x87.status.busy = 1;
        let mut cleared = suppressed;
        cleared.eip += 2;
        cleared.instruction_count = cleared.instruction_count.wrapping_add(1);
        cleared.x87.status.invalid = 0;
        cleared.x87.status.precision = 0;
        cleared.x87.status.error_summary = 0;
        cleared.x87.status.busy = 0;
        let mut stored = format.completed(cleared, true);
        stored.x87.data_offset = 0x4020;
        stored.x87.status.top = 0;
        stored.x87.tag_word = 0xffff;
        checks.check(
            "FNCLEX retains the loaded source after a suppressed push",
            &code,
            &image,
            &[
                dispatch(pushed),
                dispatch(suppressed),
                dispatch(cleared),
                Step {
                    cpu: stored,
                    ram: &[(0x8020, &format.one.to_le_bytes()[..format.bytes])],
                    exit: Exit::Dispatch(stored.eip),
                },
            ],
        );
    }
}

fn suppressed_reuse_keeps_empty_payload(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        let code = [
            load(format, 0x4000),
            vec![0xdd, 0xd8], // FSTP ST0 leaves the payload in the empty slot.
            load(format, 0x4008),
            vec![0xdb, 0xe2, 0x9b], // FNCLEX; FWAIT
        ]
        .concat();
        let mut image = stack_image(&code, 0, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x037e);
        image.map(4, 0x8000, true);
        image.data(0x8000, &format.one.to_le_bytes()[..format.bytes]);
        image.data(0x8008, &(format.infinity | 1).to_le_bytes()[..format.bytes]);
        let pushed = loaded(image.cpu, format, (TOP, 0x3fff), 0);
        let mut popped = complete_x87(pushed, 2, 0x05d8);
        popped.x87.status.top = 0;
        popped.x87.tag_word = 0xffff;
        let mut suppressed = complete_x87(popped, 6, (u16::from(format.opcode & 7) << 8) | 5);
        suppressed.x87.data_offset = 0x4008;
        suppressed.x87.status.invalid = 1;
        suppressed.x87.status.error_summary = 1;
        suppressed.x87.status.busy = 1;
        let mut cleared = suppressed;
        cleared.eip += 2;
        cleared.instruction_count = cleared.instruction_count.wrapping_add(1);
        cleared.x87.status.invalid = 0;
        cleared.x87.status.precision = 0;
        cleared.x87.status.error_summary = 0;
        cleared.x87.status.busy = 0;
        let mut waited = cleared;
        waited.eip += 1;
        waited.instruction_count = waited.instruction_count.wrapping_add(1);
        checks.check(
            "a suppressed push preserves the earlier loaded payload in its reused slot",
            &code,
            &image,
            &[
                dispatch(pushed),
                dispatch(popped),
                dispatch(suppressed),
                dispatch(cleared),
                dispatch(waited),
            ],
        );
    }
}

test_frontends!(exact_values, same_width_values);
test_frontends!(moves, register_movement);
test_frontends!(publication, loaded_values_at_faults);
test_frontends!(suppression, cleared_suppressed_load);
test_frontends!(reuse, suppressed_reuse_keeps_empty_payload);
