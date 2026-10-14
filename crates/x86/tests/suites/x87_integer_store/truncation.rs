//! FISTTP fixes rounding toward zero while retaining integer-store exception policy.

use super::*;
use crate::support::{encoding::check_length, machine::expected, step::TestModule};
use wasm86_x86::Compiler;

#[test]
fn encoding() {
    for form in TRUNCATING_FORMS {
        check_length(&form.instruction(0x4000));
    }
}

fn truncation(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for form in TRUNCATING_FORMS {
        for (source, integer, inexact) in [
            ((0, 0), 0, false),
            ((TOP, 0x3fff), 1, false),
            ((TOP, 0x3ffe), 0, true),
            ((0xc000_0000_0000_0000, 0x3fff), 1, true),
            ((0xa000_0000_0000_0001, 0x4000), 2, true),
            ((1, 0), 0, true),
        ] {
            for negative in [false, true] {
                for rc in 0..4 {
                    let source = (source.0, source.1 | if negative { 0x8000 } else { 0 });
                    let mut case = Case::masked(
                        source,
                        if negative { -integer } else { integer },
                        if inexact { PE } else { 0 },
                    );
                    // D/O/U are unmasked: none is an integer conversion exception.
                    case.control = 0x0365 | rc << 10;
                    check_store(&mut checks, form, case);
                }
            }
        }
        let mut case = Case::masked((0xc000_0000_0000_0000, 0xbfff), -1, PE | PENDING);
        case.control = 0x005f; // PC24, nearest RC, unmasked precision still stores and pops.
        check_store(&mut checks, form, case);
    }
}

fn boundaries(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (form, positive_half, negative_half, exponent) in [
        (
            TRUNCATING_FORMS[0],
            (0xffff_0000_0000_0000, 0x400d),
            Some((0x8000_8000_0000_0000, 0xc00e)),
            0x400e,
        ),
        (
            TRUNCATING_FORMS[1],
            (0xffff_ffff_0000_0000, 0x401d),
            Some((0x8000_0000_8000_0000, 0xc01e)),
            0x401e,
        ),
        (TRUNCATING_FORMS[2], (u64::MAX, 0x403d), None, 0x403e),
    ] {
        check_store(
            &mut checks,
            form,
            Case::masked(positive_half, -(form.minimum() + 1), PE),
        );
        if let Some(source) = negative_half {
            check_store(&mut checks, form, Case::masked(source, form.minimum(), PE));
        }
        for source in [
            (TOP, exponent),
            (TOP + (1_u64 << (64 - form.bytes * 8)), exponent | 0x8000),
        ] {
            for masked in [false, true] {
                let mut case = Case::masked(source, form.minimum(), 1);
                if !masked {
                    case.control = 0x037e;
                    case.output = None;
                    case.flags |= PENDING;
                }
                check_store(&mut checks, form, case);
            }
        }
    }
    check_store(
        &mut checks,
        TRUNCATING_FORMS[2],
        Case::masked((u64::MAX, 0xc03d), i64::MIN + 1, PE),
    );
    for source in [
        None,
        Some((TOP, 0x7fff)),
        Some((TOP + 1, 0x7fff)),
        Some((0xc000_0000_0000_0001, 0xffff)),
        Some((42, 0x3fff)),
    ] {
        for masked in [false, true] {
            let form = TRUNCATING_FORMS[1];
            let mut case = Case::masked(
                source.unwrap_or((TOP, 0x3fff)),
                form.minimum(),
                1 | if source.is_none() { 0x40 } else { 0 },
            );
            case.empty = source.is_none();
            if !masked {
                case.control = 0x037e;
                case.output = None;
                case.flags |= PENDING;
            }
            check_store(&mut checks, form, case);
        }
    }
}

fn memory_priority(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let form = TRUNCATING_FORMS[2];
    let code = form.instruction(0x4ffc);
    let case = Case {
        source: (TOP, 0x7fff),
        control: 0x037e,
        empty: true,
        output: None,
        flags: 0x41 | PENDING,
    };
    let mut image = initial_image(&code, &case);
    image.data(0x8ffc, &[0xa6; 4]);
    checks.check(
        "complete destination guard precedes empty-stack conversion",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::PageFault {
                address: 0x5000,
                error: 2,
            },
        }],
    );
    image.cpu.x87.status.error_summary = 1;
    image.cpu.x87.status.busy = 1;
    checks.check(
        "pending exception precedes the destination",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

fn rc_independence(engine: Engine) {
    for form in TRUNCATING_FORMS {
        let code = form.instruction(0x4000);
        let mut image = initial_image(&code, &Case::masked((0xc000_0000_0000_0000, 0x3fff), 1, PE));
        let modules: Vec<_> = (0..4)
            .map(|rc| {
                let mut cpu = image.cpu;
                cpu.x87.control.rounding_control = rc;
                Compiler::new(SegmentProfile::Flat32)
                    .specialize_on_cpu(&cpu)
                    .compile_block(0x1000, &code, 1)
                    .unwrap()
            })
            .collect();
        assert!(modules
            .windows(2)
            .all(|pair| pair[0].bytes == pair[1].bytes));
        image.cpu.x87.control.rounding_control = 2;
        let mut result = form.completed(image.cpu, 0x4000, 6);
        result.x87.status = status(0x4500 | PE);
        result.x87.tag_word = 0xffff;
        let output = 1_i64.to_le_bytes();
        assert_eq!(
            engine.observe(&TestModule::new(&modules[0]), &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: result,
                    ram: &[(0x8000, &output[..form.bytes])],
                    exit: Exit::Dispatch(result.eip)
                }]
            )
        );
    }
}

#[test]
fn no_rc_specialization() {
    rc_independence(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_no_rc_specialization() {
    rc_independence(Engine::V8);
}
test_frontends!(rounding, truncation);
test_frontends!(range_and_exceptions, boundaries);
test_frontends!(access, memory_priority);
