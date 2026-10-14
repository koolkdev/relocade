//! Deferred exceptions, restored stack positions and complete-span access checks.

use super::*;
use crate::support::x87::complete_x87;

const PROTECTED32: Format = Format {
    real: false,
    wide: true,
};

fn exception_delivery(engine: Engine, frontend: Frontend) {
    let load = PROTECTED32.instruction(true, 0x4000);
    let code = [load.as_slice(), &[0xdf, 0xe0, 0x9b]].concat();
    let mut checks = ImageSequences::new(engine, frontend, PROTECTED32.profile());
    for (control, status_in, status_out) in [
        (0x037f, 0x8081, 0x0001), // Inconsistent ES/B cannot create an exception.
        (0x037e, 0x0001, 0x8081), // An unmasked IE creates pending ES/B.
        (0x035f, 0x0020, 0x80a0), // Likewise for precision.
        (0x0340, 0x0040, 0x0040), // SF alone is not a maskable exception.
    ] {
        let mut image = PROTECTED32.image(&code);
        image.data(
            0x8000,
            &PROTECTED32.bytes([control, status_in, 0xffff, 0, 0, 0, 0]),
        );
        let mut loaded = retire(image.cpu, load.len());
        set_control(&mut loaded.x87.control, control as u16);
        loaded.x87.status = status(status_out);
        loaded.x87.tag_word = 0xffff;
        loaded.x87.instruction_offset = 0;
        loaded.x87.instruction_selector = 0;
        loaded.x87.data_offset = 0;
        loaded.x87.data_selector = 0;
        loaded.x87.opcode = 0;
        let mut observed = retire(loaded, 2);
        observed.registers.eax = (observed.registers.eax & 0xffff_0000) | u32::from(status_out);
        let waiting = if status_out & 0x80 == 0 {
            dispatch(retire(observed, 1))
        } else {
            Step {
                cpu: observed,
                ram: &[],
                exit: Exit::FloatingPoint,
            }
        };
        checks.check(
            "FLDENV defers loaded exceptions to the next waiting instruction",
            &code,
            &image,
            &[dispatch(loaded), dispatch(observed), waiting],
        );
    }
}

fn suppressed_push_then_save(engine: Engine, frontend: Frontend) {
    let save = PROTECTED32.instruction(false, 0x4000);
    let code = [&[0xd9, 0xe8][..], &save, &[0xd9, 0xc0]].concat(); // FLD1; FNSTENV; FLD ST0
    let mut image = PROTECTED32.image(&code);
    image.cpu.x87.tag_word = 0; // Full stack, so FLD1 cannot push with IM clear.
    image.cpu.x87.status = status(0x0800);
    set_control(&mut image.cpu.x87.control, 0x037e);
    let mut faulted = complete_x87(image.cpu, 2, 0x01e8);
    faulted.x87.status = status(0x8ac1);
    let mut saved = retire(faulted, save.len());
    set_control(&mut saved.x87.control, 0x037f);
    saved.x87.status = status(0x0a41);
    let bytes = PROTECTED32.bytes([
        0xffff_037e,
        0xffff_8ac1,
        0xffff_0000,
        0x1000,
        0x01e8_001b,
        0x89ab_cdef,
        0xffff_0027,
    ]);
    let mut pushed = complete_x87(saved, 2, 0x01c0);
    pushed.x87.status = status(0x0241);
    pushed.x87.tag_word = 2; // Masked overflow writes indefinite at actual ST7 (R0).
    write_register_bits(&mut pushed, 0, (0xc000_0000_0000_0000, 0xffff));
    ImageSequences::new(engine, frontend, PROTECTED32.profile()).check(
        "FNSTENV saves the suppressed push and rebases stack tracking before clearing ES",
        &code,
        &image,
        &[
            dispatch(faulted),
            Step {
                cpu: saved,
                ram: &[(0x8000, &bytes)],
                exit: Exit::Dispatch(saved.eip),
            },
            dispatch(pushed),
        ],
    );
}

fn suppressed_pop_then_save(engine: Engine, frontend: Frontend) {
    let save = PROTECTED32.instruction(false, 0x4020);
    let code = [&[0xdd, 0x1d, 0, 0x40, 0, 0][..], &save, &[0xd9, 0xc1]].concat();
    let mut image = PROTECTED32.image(&code);
    image.cpu.x87.tag_word = 0xfcff; // Only R4 (ST1) is occupied.
    image.cpu.x87.status = status(0x1800);
    set_control(&mut image.cpu.x87.control, 0x037e);
    let mut faulted = complete_x87(image.cpu, 6, 0x051d);
    faulted.x87.status = status(0x98c1);
    faulted.x87.data_offset = 0x4000;
    faulted.x87.data_selector = 0x23;
    let mut saved = retire(faulted, save.len());
    set_control(&mut saved.x87.control, 0x037f);
    saved.x87.status = status(0x1841);
    let bytes = PROTECTED32.bytes([
        0xffff_037e,
        0xffff_98c1,
        0xffff_fcff,
        0x1000,
        0x051d_001b,
        0x4000,
        0xffff_0023,
    ]);
    let mut pushed = complete_x87(saved, 2, 0x01c1);
    pushed.x87.status = status(0x1041);
    pushed.x87.tag_word = 0xfccf;
    let source = image.cpu.x87.registers[4];
    write_register_bits(&mut pushed, 2, (source.significand, source.sign_exponent));
    ImageSequences::new(engine, frontend, PROTECTED32.profile()).check(
        "FNSTENV masks a suppressed pop without advancing the next logical stack read",
        &code,
        &image,
        &[
            dispatch(faulted),
            Step {
                cpu: saved,
                ram: &[(0x8020, &bytes)],
                exit: Exit::Dispatch(saved.eip),
            },
            dispatch(pushed),
        ],
    );
}

fn loaded_top(engine: Engine, frontend: Frontend) {
    let load = PROTECTED32.instruction(true, 0x4000);
    let code = [&[0xd9, 0xe8][..], &load, &[0xd9, 0xc0]].concat();
    let mut image = PROTECTED32.image(&code);
    image.cpu.x87.status = status(0);
    image.cpu.x87.tag_word = 0xffff;
    // TOP=3, only physical R7 nonempty. The earlier FLD1 supplies R7's value.
    image.data(
        0x8000,
        &PROTECTED32.bytes([0x037f, 0x1800, 0x3fff, 0x1234, 0x0567_0023, 0x5678, 0x0033]),
    );
    let mut pushed = complete_x87(image.cpu, 2, 0x01e8);
    pushed.x87.status = status(0x3800);
    pushed.x87.tag_word = 0x3fff;
    write_register_bits(&mut pushed, 7, (0x8000_0000_0000_0000, 0x3fff));
    let mut loaded = retire(pushed, load.len());
    loaded.x87.status = status(0x1800);
    loaded.x87.opcode = 0x0567;
    loaded.x87.instruction_offset = 0x1234;
    loaded.x87.instruction_selector = 0x0023;
    loaded.x87.data_offset = 0x5678;
    loaded.x87.data_selector = 0x0033;
    let mut faulted = complete_x87(loaded, 2, 0x01c0);
    faulted.x87.status = status(0x1041);
    faulted.x87.tag_word = 0x3fef;
    write_register_bits(&mut faulted, 2, (0xc000_0000_0000_0000, 0xffff));
    ImageSequences::new(engine, frontend, PROTECTED32.profile()).check(
        "FLDENV changes TOP while preserving earlier register writes and physical tags",
        &code,
        &image,
        &[dispatch(pushed), dispatch(loaded), dispatch(faulted)],
    );
}

fn memory_faults(engine: Engine, frontend: Frontend) {
    for format in Format::ALL.into_iter().filter(|format| !format.real) {
        for load in [false, true] {
            let bytes = if format.wide { 28 } else { 14 };
            let code = format.instruction(load, 0x5001 - bytes);
            let mut image = format.image(&code);
            if !load {
                set_control(&mut image.cpu.x87.control, 0x037e);
                image.cpu.x87.status = status(0x8081);
            }
            let mut checks = ImageSequences::new(engine, frontend, format.profile());
            checks.check(
                "the final image byte faults before any memory or x87 change",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::PageFault {
                        address: 0x5000,
                        error: if load { 0 } else { 2 },
                    },
                }],
            );
            if load {
                image.cpu.x87.status = status(0x8081);
                set_control(&mut image.cpu.x87.control, 0x037e);
                checks.check(
                    "a pending exception precedes FLDENV's memory access",
                    &code,
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: Exit::FloatingPoint,
                    }],
                );
            }

            let code = format.instruction(load, 0x4000);
            let mut image = format.image(&code);
            image.cpu.segments.ds.limit = 0x4000 + bytes - 2;
            ImageSequences::new(engine, frontend, SegmentProfile::Segmented32).check(
                "segment limits cover the full environment image",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                }],
            );
        }
    }
}

test_frontends!(loaded_exception_delivery, exception_delivery);
test_frontends!(masked_suppressed_stack, suppressed_push_then_save);
test_frontends!(masked_suppressed_pop, suppressed_pop_then_save);
test_frontends!(restored_top, loaded_top);
test_frontends!(environment_faults, memory_faults);
