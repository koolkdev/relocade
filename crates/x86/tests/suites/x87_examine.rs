//! FXAM observes class and sign without consuming or normalizing the operand.

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Step},
    x87::{complete_x87, dispatch, set_control, stack_image, write_register_bits, write_value},
};
use relocade_x86::{CpuState, SegmentProfile};

fn condition_codes(cpu: &mut CpuState, word: u16) {
    cpu.x87.status.c0 = ((word >> 8) & 1) as u8;
    cpu.x87.status.c1 = ((word >> 9) & 1) as u8;
    cpu.x87.status.c2 = ((word >> 10) & 1) as u8;
    cpu.x87.status.c3 = ((word >> 14) & 1) as u8;
}

fn raw_classes(frontend: Frontend) {
    let code = [0xd9, 0xe5, 0x9b]; // FXAM; FWAIT
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (name, significand, exponent, codes) in [
        ("zero", 0, 0, 0x4000),
        ("denormal", 1, 0, 0x4400),
        ("pseudo-denormal", 0x8000_0000_0000_0042, 0, 0x4400),
        ("normal", 0x8000_0000_0000_0042, 0x3fff, 0x0400),
        ("minimum normal", 0x8000_0000_0000_0000, 1, 0x0400),
        ("maximum finite", 0xffff_ffff_ffff_ffff, 0x7ffe, 0x0400),
        ("infinity", 0x8000_0000_0000_0000, 0x7fff, 0x0500),
        ("quiet NaN", 0xc000_0000_0000_0042, 0x7fff, 0x0100),
        ("signaling NaN", 0x8000_0000_0000_0042, 0x7fff, 0x0100),
        ("unnormal", 0x0000_0000_0000_0042, 0x3fff, 0),
        ("unnormal zero significand", 0, 1, 0),
        ("pseudo-infinity", 0, 0x7fff, 0),
        ("pseudo-NaN", 0x4000_0000_0000_0042, 0x7fff, 0),
    ] {
        for sign in [0, 0x8000] {
            for empty in [false, true] {
                let mut image = stack_image(&code, 5, 0xffff);
                // No new numerical exception or stack fault is permitted,
                // even for SNaNs, unsupported encodings and empty entries.
                set_control(&mut image.cpu.x87.control, 0x0300);
                image.cpu.x87.status.precision = 0;
                image.cpu.x87.status.c0 = 0x81;
                image.cpu.x87.status.c1 = 0x83;
                image.cpu.x87.status.c2 = 0x85;
                image.cpu.x87.status.c3 = 0x87;
                write_value(&mut image.cpu, 5, (significand, exponent | sign));
                if empty {
                    image.cpu.x87.tag_word |= 3 << 10;
                }
                let mut examined = complete_x87(image.cpu, 2, 0x01e5);
                let codes = if empty { 0x4100 } else { codes };
                condition_codes(&mut examined, codes | if sign == 0 { 0 } else { 0x0200 });
                let mut waited = examined;
                waited.eip += 1;
                waited.instruction_count = waited.instruction_count.wrapping_add(1);
                checks.check(
                    &format!("{name}, sign={sign:x}, empty={empty}"),
                    &code,
                    &image,
                    &[dispatch(examined), dispatch(waited)],
                );
            }
        }
    }

    for tag in [0, 1, 2] {
        let mut image = stack_image(&code, 0, 0xfffc | tag);
        write_register_bits(&mut image.cpu, 0, (0, 0x8000));
        let mut examined = complete_x87(image.cpu, 2, 0x01e5);
        condition_codes(&mut examined, 0x4200);
        let mut waited = examined;
        waited.eip += 1;
        waited.instruction_count = waited.instruction_count.wrapping_add(1);
        checks.check(
            "a nonempty tag does not replace numerical classification",
            &code,
            &image,
            &[dispatch(examined), dispatch(waited)],
        );
    }
}

fn loaded_binary_values(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for (width, source, extended, codes) in [
        (4, 0x3f80_0000, (0x8000_0000_0000_0000, 0x3fff), 0x0400),
        (4, 0x8000_0000, (0, 0x8000), 0x4200),
        (4, 0x7f80_0000, (0x8000_0000_0000_0000, 0x7fff), 0x0500),
        (4, 0x7fc0_0001, (0xc000_0100_0000_0000, 0x7fff), 0x0100),
        (
            8,
            0x3ff0_0000_0000_0000,
            (0x8000_0000_0000_0000, 0x3fff),
            0x0400,
        ),
        (8, 0x8000_0000_0000_0000, (0, 0x8000), 0x4200),
        (
            8,
            0x7ff0_0000_0000_0000,
            (0x8000_0000_0000_0000, 0x7fff),
            0x0500,
        ),
        (
            8,
            0x7ff8_0000_0000_0001,
            (0xc000_0000_0000_0800, 0x7fff),
            0x0100,
        ),
    ] {
        let opcode = if width == 4 { 0xd9 } else { 0xdd };
        let code = [opcode, 0x05, 0, 0x40, 0, 0, 0xd9, 0xe5];
        let mut image = stack_image(&code, 0, 0xffff);
        image.map(4, 0x8000, false);
        image.data(0x8000, &u64::to_le_bytes(source)[..width]);
        let mut loaded = complete_x87(image.cpu, 6, if width == 4 { 0x0105 } else { 0x0505 });
        loaded.x87.data_offset = 0x4000;
        loaded.x87.data_selector = 0x23;
        loaded.x87.status.top = 7;
        loaded.x87.status.c1 = 0;
        write_value(&mut loaded, 7, extended);
        let mut examined = complete_x87(loaded, 2, 0x01e5);
        condition_codes(&mut examined, codes);
        checks.check(
            &format!("loaded binary{} {source:x}", width * 8),
            &code,
            &image,
            &[dispatch(loaded), dispatch(examined)],
        );
    }
}

fn constants_and_freed_payload(frontend: Frontend) {
    // FLD1; FXAM; FLDZ; FCHS; FXAM; FFREE ST(0); FXAM.
    let code = [
        0xd9, 0xe8, 0xd9, 0xe5, 0xd9, 0xee, 0xd9, 0xe0, 0xd9, 0xe5, 0xdd, 0xc0, 0xd9, 0xe5,
    ];
    let image = stack_image(&code, 0, 0xffff);
    let mut one = complete_x87(image.cpu, 2, 0x01e8);
    one.x87.status.top = 7;
    one.x87.status.c1 = 0;
    write_value(&mut one, 7, (0x8000_0000_0000_0000, 0x3fff));
    let mut examined_one = complete_x87(one, 2, 0x01e5);
    condition_codes(&mut examined_one, 0x0400);
    let mut zero = complete_x87(examined_one, 2, 0x01ee);
    zero.x87.status.top = 6;
    write_value(&mut zero, 6, (0, 0));
    let mut negative_zero = complete_x87(zero, 2, 0x01e0);
    write_value(&mut negative_zero, 6, (0, 0x8000));
    let mut examined_zero = complete_x87(negative_zero, 2, 0x01e5);
    condition_codes(&mut examined_zero, 0x4200);
    let mut freed = complete_x87(examined_zero, 2, 0x05c0);
    freed.x87.tag_word |= 3 << 12;
    let mut examined_empty = complete_x87(freed, 2, 0x01e5);
    condition_codes(&mut examined_empty, 0x4300);
    ImageSequences::new(frontend, SegmentProfile::Flat32).check(
        "examine retained constants and the sign left in an empty register",
        &code,
        &image,
        &[
            dispatch(one),
            dispatch(examined_one),
            dispatch(zero),
            dispatch(negative_zero),
            dispatch(examined_zero),
            dispatch(freed),
            dispatch(examined_empty),
        ],
    );
}

fn pending_exception(frontend: Frontend) {
    let code = [0xd9, 0xe5];
    let mut image = stack_image(&code, 0, 0xffff);
    set_control(&mut image.cpu.x87.control, 0x037e);
    image.cpu.x87.status.invalid = 1;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    ImageSequences::new(frontend, SegmentProfile::Flat32).check(
        "a pending exception prevents FXAM's condition codes and metadata",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

#[test]
fn encoding() {
    crate::support::encoding::check_length(&[0xd9, 0xe5]);
}

test_frontends!(classes, raw_classes);
test_frontends!(loaded_values, loaded_binary_values);
test_frontends!(retained_values, constants_and_freed_payload);
test_frontends!(pending, pending_exception);
