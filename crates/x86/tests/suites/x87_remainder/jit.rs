//! Native quotient estimates, preserved precision, and precise restart boundaries.

use super::*;
use crate::support::{machine::expected, step::TestModule, x87::INDEFINITE};
use wasm86_x86::Compiler;
use wasmparser::{Operator, Parser, Payload};

fn binary64_bits(value: f64) -> (u64, u16) {
    let bits = value.to_bits();
    let sign = ((bits >> 48) as u16) & 0x8000;
    if value == 0.0 {
        (0, sign)
    } else {
        let exponent = ((bits >> 52) & 0x7ff) as u16;
        assert!(exponent != 0 && exponent != 0x7ff);
        (
            ((bits & ((1 << 52) - 1)) | (1 << 52)) << 11,
            sign | (exponent + 15360),
        )
    }
}

fn native_quotient(opcode: u8) {
    let fop = 0x0100 | u16::from(opcode);
    let code = [
        0xdd, 0x05, 0, 0x40, 0, 0, // FLD divisor
        0xdd, 0x05, 8, 0x40, 0, 0, // FLD dividend
        0xd9, opcode,
    ];
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .specialize_on_cpu(&CpuState::default())
        .compile_block(0x1000, &code, 3)
        .unwrap();
    assert_eq!(
        Parser::new(0)
            .parse_all(&compiled.bytes)
            .filter_map(|payload| match payload.unwrap() {
                Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
                _ => None,
            })
            .flat_map(|reader| reader.into_iter().map(Result::unwrap))
            .filter(|op| matches!(op, Operator::F64Div))
            .count(),
        1,
    );
    let module = TestModule::new(&compiled);
    let mut cases = vec![
        (6.5_f64, 2.0_f64, 0.5_f64, 3, false),
        (-6.5, -2.0, -0.5, 3, false),
        (0.25, 3.0, 0.25, 0, false),
        (-0.0, 1.0, -0.0, 0, false),
        (2_f64.powi(63), 1.5, 0.5, 5, false),
        (2_f64.powi(64), 1.5, 2_f64.powi(32), 0, true),
    ];
    if opcode == 0xf5 {
        cases.extend([
            (3.0, 2.0, -1.0, 2, false),
            (5.0, 2.0, 1.0, 2, false),
            (0.75, 1.0, -0.25, 1, false),
            (16.75, 2.5, -0.75, 7, false),
            (-16.75, -2.5, 0.75, 7, false),
        ]);
    }
    for (dividend, divisor, remainder, quotient, incomplete) in cases {
        for (pc, rc) in [(0, 0), (1, 1), (2, 2), (3, 3)] {
            let mut image = stack_image(&code, 0, 0xffff);
            set_control(&mut image.cpu.x87.control, 0x007f | (pc << 8) | (rc << 10));
            image.cpu.x87.status.precision = 0;
            image.map(4, 0x8000, false);
            image.data(0x8000, &divisor.to_le_bytes());
            image.data(0x8008, &dividend.to_le_bytes());
            let mut result = complete_x87(image.cpu, 6, 0x0505);
            result = complete_x87(result, 6, 0x0505);
            result = complete_x87(result, 2, fop);
            result.x87.status.top = 6;
            result.x87.status.c1 = 0; // FLD clears C1 before a partial reduction.
            result.x87.data_offset = 0x4008;
            result.x87.data_selector = 0x23;
            if incomplete {
                result.x87.status.c2 = 1;
            } else {
                quotient_flags(&mut result, quotient);
            }
            write_value(&mut result, 7, binary64_bits(divisor));
            write_value(&mut result, 6, binary64_bits(remainder));
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(&image, &[dispatch(result)]),
                "{dividend} / {divisor}, PC={pc}, RC={rc}"
            );
        }
    }
}

fn restart(opcode: u8) {
    let code = [0xd9, opcode];
    let fop = 0x0100 | u16::from(opcode);
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &code, 1)
        .unwrap();
    let block = TestModule::new(&compiled);
    let linked = TestModule::new(&compiled).with_interpreter(TestModule::interpreter());
    for (left, right, output, quotient, flags) in [
        ((LEAD, 0x7fff), (LEAD, 0x3fff), INDEFINITE, 0, 1),
        ((LEAD, 0x3fff), (0, 0), INDEFINITE, 0, 1),
        ((1, 0), (LEAD, 1), (1, 0), 0, 2),
        ((LEAD + 1, 1), (LEAD, 1), (1, 0), 1, 0),
    ] {
        let mut image = stack_image(&code, 0, 0xffff);
        image.cpu.x87.status.precision = 0;
        write_value(&mut image.cpu, 0, left);
        write_value(&mut image.cpu, 1, right);
        image.check_unchanged_exit(
            &block,
            "remainder restarts before current effects",
            Exit::Interpret,
        );
        let mut result = complete_x87(image.cpu, 2, fop);
        quotient_flags(&mut result, quotient);
        result.x87.status.invalid = u8::from(flags == 1);
        result.x87.status.denormal = u8::from(flags == 2);
        write_value(&mut result, 0, output);
        assert_eq!(
            linked.observe(&image.input(), 1),
            expected(&image, &[dispatch(result)])
        );
    }

    // A failed remainder guard keeps the two earlier pushes exactly once.
    let code = [0xd9, 0xee, 0xd9, 0xe8, 0xd9, opcode]; // FLDZ; FLD1; remainder
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x1000, &code, 3)
        .unwrap();
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.x87.status.precision = 0;
    let mut prefix = complete_x87(image.cpu, 2, 0x01ee);
    prefix = complete_x87(prefix, 2, 0x01e8);
    prefix.x87.status.top = 6;
    prefix.x87.status.c1 = 0;
    write_value(&mut prefix, 7, (0, 0));
    write_value(&mut prefix, 6, (LEAD, 0x3fff));
    assert_eq!(
        TestModule::new(&compiled).observe(&image.input(), 1),
        expected(
            &image,
            &[crate::support::machine::Step {
                cpu: prefix,
                ram: &[],
                exit: Exit::Interpret,
            }]
        ),
    );
    let mut result = complete_x87(prefix, 2, fop);
    result.x87.status.invalid = 1;
    quotient_flags(&mut result, 0);
    write_value(&mut result, 6, INDEFINITE);
    assert_eq!(
        TestModule::new(&compiled)
            .with_interpreter(TestModule::interpreter())
            .observe(&image.input(), 1),
        expected(&image, &[dispatch(result)]),
    );
}

#[test]
fn native_division_and_control_independence() {
    for opcode in [0xf8, 0xf5] {
        native_quotient(opcode);
    }
}

#[test]
fn exceptional_operands_and_tiny_results_restart() {
    for opcode in [0xf8, 0xf5] {
        restart(opcode);
    }
}

fn preserves_native_precision(opcode: u8) {
    let code = [
        0xdd, 0x05, 0, 0x40, 0, 0, 0xdd, 0x05, 8, 0x40, 0, 0, 0xd9, opcode, 0xdc, 0x05, 0, 0x40, 0,
        0,
    ];
    let mut cpu = CpuState::default();
    set_control(&mut cpu.x87.control, 0x027f);
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .specialize_on_cpu(&cpu)
        .compile_block(0x1000, &code, 4)
        .unwrap();
    assert!(Parser::new(0).parse_all(&compiled.bytes).any(|payload| {
        matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if body.get_operators_reader().unwrap()
            .into_iter().any(|op| matches!(op.unwrap(), Operator::F64Add)))
    }));
}

#[test]
fn remainder_preserves_native_precision_for_following_arithmetic() {
    for opcode in [0xf8, 0xf5] {
        preserves_native_precision(opcode);
    }
}
