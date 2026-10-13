//! Integer memory operands expand exactly before the shared arithmetic operation.

#[path = "integer/jit.rs"]
mod jit;

use super::*;

fn forms_and_signed_sources(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [Source::Integer16(-3), Source::Integer32(-3)] {
        for (extension, result) in [
            (0, (0xc000_0000_0000_0000, 0x4000)), // 6 + -3 = 3
            (1, (0x9000_0000_0000_0000, 0xc003)), // 6 * -3 = -18
            (4, (0x9000_0000_0000_0000, 0x4002)), // 6 - -3 = 9
            (5, (0x9000_0000_0000_0000, 0xc002)), // -3 - 6 = -9
            (6, (LEADING, 0xc000)),               // 6 / -3 = -2
            (7, (LEADING, 0xbffe)),               // -3 / 6 = -0.5
        ] {
            check_case(
                &mut checks,
                "signed source, operand order and unchanged TOP",
                Case {
                    source,
                    extension,
                    left: (0xc000_0000_0000_0000, 0x4001),
                    control: 0x037f,
                    result: Some(result),
                    flags: 0,
                    empty: false,
                },
            );
        }
    }
    for (source, result) in [
        (Source::Integer16(i16::MIN), (LEADING, 0xc00e)),
        (Source::Integer16(i16::MAX), (0xfffe_0000_0000_0000, 0x400d)),
        (Source::Integer32(i32::MIN), (LEADING, 0xc01e)),
        (Source::Integer32(i32::MAX), (0xffff_fffe_0000_0000, 0x401d)),
    ] {
        // An operand-size prefix cannot narrow either integer encoding. End
        // the source at the page boundary to detect an incorrect wider read.
        let address = 0x5000 - source.bytes().len() as u32;
        let code = [vec![0x66], source.instruction(0, address)].concat();
        let mut image = initial_image(&code, source, (0, 0));
        image.data(address + 0x4000, &source.bytes());
        let mut cpu = completed_memory(image.cpu, source, 0);
        cpu.eip += 1;
        cpu.x87.data_offset = address;
        write_value(&mut cpu, 3, result);
        checks.check(
            "fixed-width signed integer limits",
            &code,
            &image,
            &[dispatch(cpu)],
        );
    }
}

fn exact_expansion_and_rounding(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    check_case(
        &mut checks,
        "PC24 applies after exact integer expansion",
        Case {
            source: Source::Integer32(16_777_217),
            extension: 0,
            left: (LEADING, 0xc017),
            control: 0x007f,
            result: Some((LEADING, 0x3fff)),
            flags: 0,
            empty: false,
        },
    );
    for (control, result, flags) in [
        (0x007f, 0xaaaa_ab00_0000_0000, 0x220),
        (0x0c7f, 0xaaaa_aa00_0000_0000, 0x20),
        (0x027f, 0xaaaa_aaaa_aaaa_a800, 0x20),
        (0x037f, 0xaaaa_aaaa_aaaa_aaab, 0x220),
    ] {
        check_case(
            &mut checks,
            "integer division obeys precision and rounding",
            Case {
                source: Source::Integer16(3),
                extension: 6,
                left: (LEADING, 0x3fff),
                control,
                result: Some((result, 0x3ffd)),
                flags,
                empty: false,
            },
        );
    }
    for (control, result, flags) in [
        (0x007f, LEADING, 0x20),
        (0x087f, LEADING + (1 << 40), 0x220),
        (0x005f, LEADING, 0x20), // Unmasked precision still writes the result.
    ] {
        check_case(
            &mut checks,
            "zero integer does not bypass result rounding",
            Case {
                source: Source::Integer32(0),
                extension: 0,
                left: (LEADING + 1, 0x3fff),
                control,
                result: Some((result, 0x3fff)),
                flags,
                empty: false,
            },
        );
    }
    for extension in [1, 7] {
        check_case(
            &mut checks,
            "integer zero is positive before arithmetic",
            Case {
                source: Source::Integer16(0),
                extension,
                left: (LEADING, 0xbfff),
                control: 0x037f,
                result: Some((0, 0x8000)),
                flags: 0,
                empty: false,
            },
        );
    }
}

fn exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for control in [0x037f, 0x0378] {
        for (source, extension, left, value, flags, empty) in [
            (0, 6, (LEADING, 0xbfff), (LEADING, 0xffff), 4, false),
            (1, 7, (0, 0x8000), (LEADING, 0xffff), 4, false),
            (0, 6, (0, 0), INDEFINITE, 1, false),
            (
                1,
                0,
                (LEADING + 1, 0x7fff),
                (0xc000_0000_0000_0001, 0x7fff),
                1,
                false,
            ),
            (1, 0, (1, 0x3fff), INDEFINITE, 1, false),
            (1, 1, (1, 0), (1, 0), 2, false),
            (0, 6, (LEADING, 0x3fff), INDEFINITE, 0x41, true),
        ] {
            check_case(
                &mut checks,
                "integer arithmetic preserves exception suppression",
                Case {
                    source: Source::Integer32(source),
                    extension,
                    left,
                    control,
                    result: (flags & !control & 0x3f == 0).then_some(value),
                    flags,
                    empty,
                },
            );
        }
    }
    let nan = (0xc000_0000_0000_0042, 0xffff);
    check_case(
        &mut checks,
        "quiet NaN survives integer arithmetic without invalid",
        Case {
            source: Source::Integer16(1),
            extension: 0,
            left: nan,
            control: 0x037e,
            result: Some(nan),
            flags: 0,
            empty: false,
        },
    );
}

fn memory_before_effects(engine: Engine, frontend: Frontend) {
    let source = Source::Integer32(0);
    let code = source.instruction(6, 0x4ffe);
    let mut image = initial_image(&code, source, (0, 0));
    image.cpu.x87.tag_word = 0xffff;
    set_control(&mut image.cpu.x87.control, 0x037e);
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    checks.check(
        "complete source read precedes stack and numerical exceptions",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x5000,
                error: 0,
            },
        }],
    );
    image.cpu.x87.status.invalid = 1;
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "pending exception precedes the integer memory read",
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
fn encodings() {
    for source in [Source::Integer16(0), Source::Integer32(0)] {
        for extension in [0, 1, 4, 5, 6, 7] {
            crate::support::encoding::check_length(&source.instruction(extension, 0x4000));
        }
    }
}

test_frontends!(forms, forms_and_signed_sources);
test_frontends!(rounding, exact_expansion_and_rounding);
test_frontends!(faults, exceptions);
test_frontends!(memory, memory_before_effects);
