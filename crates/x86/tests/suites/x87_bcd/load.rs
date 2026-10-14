//! FBLD is exact, preserves the separate sign, and shares ordinary push faults.

use super::*;
use crate::support::{machine::expected, step::TestModule, x87::INDEFINITE};
use wasm86_x86::compile_block_from_bytes;

fn instruction(address: u32) -> Vec<u8> {
    [&[0xdf, 0x25][..], &address.to_le_bytes()].concat()
}

fn check_conversion(checks: &mut ImageSequences, bcd: [u8; 10], value: (u64, u16), control: u16) {
    let code = [instruction(0x4001), vec![0xdb, 0x3d, 0x20, 0x40, 0, 0]].concat();
    let mut image = image(&code, 0, 0xffff);
    set_control(&mut image.cpu.x87.control, control);
    image.data(0x8001, &bcd);
    image.data(0x801f, &[0xa6; 12]);
    let mut loaded = completed(image.cpu, 6, 0x0725, 0x4001);
    loaded.x87.status = status(0x7d00);
    loaded.x87.tag_word = if value.0 == 0 { 0x7fff } else { 0x3fff };
    write_register_bits(&mut loaded, 7, value);
    let mut stored = completed(loaded, 6, 0x033d, 0x4020);
    stored.x87.status.top = 0;
    stored.x87.tag_word = 0xffff;
    let output = real80(value);
    checks.check(
        &format!("FBLD {bcd:02x?} is exact under control {control:04x}"),
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

fn exact_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (bcd, value) in [
        ([0; 10], (0, 0)),
        ([0, 0, 0, 0, 0, 0, 0, 0, 0, 0x7f], (0, 0)),
        ([0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff], (0, 0x8000)),
        (
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            (0x8000_0000_0000_0000, 0x3fff),
        ),
        (
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0x80],
            (0x8000_0000_0000_0000, 0xbfff),
        ),
        (
            [0x78, 0x56, 0x34, 0x12, 0x90, 0x78, 0x56, 0x34, 0x12, 0],
            (0xdb4d_a5d3_1879_a700, 0x4037),
        ),
        (
            [0x93, 0x09, 0x74, 0x54, 0x92, 0x19, 0x07, 0x90, 0, 0],
            (0x8000_0000_0000_0400, 0x4034),
        ),
        (MAXIMUM, (0xde0b_6b3a_763f_fff0, 0x403a)),
    ] {
        check_conversion(&mut checks, bcd, value, 0x037f);
    }
    for precision in [0, 0x0200, 0x0300] {
        for rounding in [0, 0x0400, 0x0800, 0x0c00] {
            let mut negative_maximum = MAXIMUM;
            negative_maximum[9] = 0x80;
            // An unmasked precision exception must not narrow these 60-bit integers.
            check_conversion(
                &mut checks,
                negative_maximum,
                (0xde0b_6b3a_763f_fff0, 0xc03a),
                0x005f | precision | rounding,
            );
        }
    }
}

fn memory_span(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let code = [vec![0x66], instruction(0x4ff6)].concat();
    let mut image = image(&code, 0, 0xffff);
    image.map(4, 0x8000, false);
    image.data(0x8ff6, &MAXIMUM);
    let mut loaded = completed(image.cpu, 7, 0x0725, 0x4ff6);
    loaded.x87.status = status(0x7d00);
    loaded.x87.tag_word = 0x3fff;
    write_register_bits(&mut loaded, 7, (0xde0b_6b3a_763f_fff0, 0x403a));
    checks.check(
        "66 keeps FBLD's exact ten-byte operand",
        &code,
        &image,
        &[dispatch(loaded)],
    );

    let code = instruction(0x4ffb);
    let mut image = super::image(&code, 0, 0xffff);
    image.map(5, 0xc000, false);
    let mut bcd = MAXIMUM;
    bcd[9] = 0xff;
    image.data(0x8ffb, &bcd[..5]);
    image.data(0xc000, &bcd[5..]);
    let mut loaded = completed(image.cpu, 6, 0x0725, 0x4ffb);
    loaded.x87.status = status(0x7d00);
    loaded.x87.tag_word = 0x3fff;
    write_register_bits(&mut loaded, 7, (0xde0b_6b3a_763f_fff0, 0xc03a));
    checks.check(
        "FBLD reads digits and sign across distinct physical frames",
        &code,
        &image,
        &[dispatch(loaded)],
    );

    let code = instruction(0x4ff7);
    let mut image = super::image(&code, 0, 0);
    set_control(&mut image.cpu.x87.control, 0x037e);
    for (pending, exit) in [
        (
            false,
            Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        ),
        (true, Exit::FloatingPoint),
    ] {
        image.cpu.x87.status.invalid = u8::from(pending);
        image.cpu.x87.status.error_summary = u8::from(pending);
        image.cpu.x87.status.busy = u8::from(pending);
        checks.check(
            "FBLD waits and validates the complete source before stack overflow",
            &code,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit,
            }],
        );
    }
}

fn stack_overflow(engine: Engine, frontend: Frontend) {
    let load = instruction(0x4001);
    let code = [load.as_slice(), &[0x9b]].concat();
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let linked = TestModule::new(&compile_block_from_bytes(0x1000, &load, 1).unwrap())
        .with_interpreter(TestModule::interpreter());
    for masked in [false, true] {
        let mut image = image(&code, 0, 0);
        image.data(0x8001, &MAXIMUM);
        set_control(
            &mut image.cpu.x87.control,
            if masked { 0x037f } else { 0x037e },
        );
        let mut loaded = completed(image.cpu, 6, 0x0725, 0x4001);
        loaded.x87.status = status(if masked { 0x7f41 } else { 0xc7c1 });
        if masked {
            loaded.x87.tag_word = 0x8000;
            write_register_bits(&mut loaded, 7, INDEFINITE);
        }
        let mut waited = loaded;
        if masked {
            waited.eip += 1;
            waited.instruction_count = waited.instruction_count.wrapping_add(1);
        }
        checks.check(
            "FBLD stack overflow suppresses only an unmasked invalid push",
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: waited,
                    ram: &[],
                    exit: if masked {
                        Exit::Dispatch(waited.eip)
                    } else {
                        Exit::FloatingPoint
                    },
                },
            ],
        );
        if matches!(frontend, Frontend::Block) {
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(loaded)]),
                "the linked interpreter completes a restarted FBLD"
            );
        }
    }
}

test_frontends!(fbld_exact, exact_values);
test_frontends!(fbld_memory, memory_span);
test_frontends!(fbld_overflow, stack_overflow);
