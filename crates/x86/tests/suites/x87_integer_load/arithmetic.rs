//! Integer loads preserve exact inputs for the first arithmetic operation.

use super::*;
use relocade_x86::Compiler;
use wasmparser::{Operator, Parser, Payload};

#[test]
fn first_product_uses_exact_integer_load() {
    use Source::{Dword, Qword, Word};
    for (source, product, precision, incremented) in [
        (Word(0), (0, 0), 0, 0),
        (Word(1), (0x8000_0000_0000_0800, 0x3fff), 0, 0),
        (Word(-1), (0x8000_0000_0000_0800, 0xbfff), 0, 0),
        (Word(i16::MIN), (0x8000_0000_0000_0800, 0xc00e), 0, 0),
        (Word(i16::MAX), (0xfffe_0000_0000_1000, 0x400d), 1, 1),
        (Dword(0), (0, 0), 0, 0),
        (Dword(i32::MIN), (0x8000_0000_0000_0800, 0xc01e), 0, 0),
        (Dword(i32::MAX), (0xffff_fffe_0000_1000, 0x401d), 1, 1),
        (Dword(0x0100_0001), (0x8000_0080_0000_0800, 0x4017), 1, 0),
        (Dword(-0x0100_0001), (0x8000_0080_0000_0800, 0xc017), 1, 0),
        (Qword(0), (0, 0), 0, 0),
        // Rounding these inputs to binary64 before multiplication changes the
        // result. The exact product is (2^53 + 1) * (1 + 2^-52).
        (
            Qword(0x0020_0000_0000_0001),
            (0x8000_0000_0000_1000, 0x4034),
            1,
            1,
        ),
        (
            Qword(-0x0020_0000_0000_0001),
            (0x8000_0000_0000_1000, 0xc034),
            1,
            1,
        ),
        (Qword(i64::MIN), (0x8000_0000_0000_0800, 0xc03e), 0, 0),
        (Qword(i64::MAX), (0x8000_0000_0000_0800, 0x403e), 1, 1),
    ] {
        let code = [
            source.instruction(0x4000),
            vec![0xdc, 0x0d, 8, 0x40, 0, 0], // FMUL m64
        ]
        .concat();
        let mut observed = CpuState::default();
        set_control(&mut observed.x87.control, 0x027f);
        let module = TestModule::new(
            &Compiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&observed)
                .compile_block(0x1000, &code, 2)
                .unwrap(),
        );
        let native = Parser::new(0).parse_all(module.bytes()).any(|payload| {
            matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if
                body.get_operators_reader().unwrap().into_iter().any(|op| matches!(op.unwrap(), Operator::F64Mul)))
        });
        assert_eq!(native, !matches!(source, Qword(_)), "{source:?}");
        for negative_factor in [false, true] {
            let mut image = initial_image(&code, source, 0xffff);
            image.cpu.x87.control = observed.x87.control;
            let factor = 0x3ff0_0000_0000_0001_u64 | (u64::from(negative_factor) << 63);
            image.data(0x8008, &factor.to_le_bytes());
            let mut result = source.completed_load(image.cpu, 0x4000, 6);
            result = complete_x87(result, 6, 0x040d);
            result.x87.status.top = 7;
            result.x87.status.precision = precision;
            result.x87.status.c1 = incremented;
            result.x87.tag_word = if product.0 == 0 { 0x7fff } else { 0x3fff };
            result.x87.data_offset = 0x4008;
            write_register_bits(
                &mut result,
                7,
                (
                    product.0,
                    product.1 ^ if negative_factor { 0x8000 } else { 0 },
                ),
            );
            assert_eq!(
                module.observe(&image.input(), 1),
                expected(&image, &[dispatch(result)]),
                "FILD {source:?}, negative factor {negative_factor}"
            );
        }
    }
}

#[test]
fn integer_product_and_sum_respect_each_precision_boundary() {
    let code = [
        0xdb, 0x05, 0, 0x40, 0, 0, // FILD m32
        0xda, 0x0d, 4, 0x40, 0, 0, // FIMUL m32
        0xda, 0x05, 8, 0x40, 0, 0, // FIADD m32
        0xdf, 0x3d, 16, 0x40, 0, 0, // FISTP m64
    ];
    for (control, a, b, c, stored, extended, inexact) in [
        // The first product at PC24 already lost its low bit before the add.
        (0x007f, 16_777_217, 1_i32, -16_777_216_i32, 0_i64, (0, 0), 1),
        (0x027f, 16_777_217, 1, -16_777_216, 1, (1 << 63, 0x3fff), 0),
        // PC53 loses the addend. PC64 retains it without double rounding.
        (0x027f, i32::MIN, i32::MIN, 1, 1 << 62, (1 << 63, 0x403d), 1),
        (
            0x037f,
            i32::MIN,
            i32::MIN,
            1,
            (1 << 62) + 1,
            ((1 << 63) + 2, 0x403d),
            0,
        ),
        (0x037f, -3, 4, 1, -11, (0xb000_0000_0000_0000, 0xc002), 0),
    ] {
        let source = Source::Dword(a);
        let mut image = initial_image(&code, source, 0xffff);
        set_control(&mut image.cpu.x87.control, control);
        image.data(0x8004, &b.to_le_bytes());
        image.data(0x8008, &c.to_le_bytes());
        let module = TestModule::new(
            &Compiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&image.cpu)
                .compile_block(0x1000, &code, 4)
                .unwrap(),
        );
        if control == 0x037f {
            let mut multiplies = 0;
            for payload in Parser::new(0).parse_all(module.bytes()) {
                if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                    multiplies += body
                        .get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .filter(|op| matches!(op, Ok(Operator::I64Mul)))
                        .count();
                }
            }
            assert_eq!(multiplies, 1, "bounded PC64 product uses one i64.mul");
        }
        let mut result = source.completed_load(image.cpu, 0x4000, 6);
        result = complete_x87(result, 6, 0x020d);
        result = complete_x87(result, 6, 0x0205);
        result = complete_x87(result, 6, 0x073d);
        result.x87.status.c1 = 0;
        result.x87.status.precision = inexact;
        result.x87.data_offset = 0x4010;
        write_register_bits(&mut result, 7, extended);
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: result,
                    ram: &[(0x8010, &stored.to_le_bytes())],
                    exit: Exit::Dispatch(result.eip),
                }]
            ),
            "control {control:04x}, ({a} * {b}) + {c}"
        );
    }
}

#[test]
fn integer_product_preserves_signed_zero_through_sign_change() {
    for negate in [false, true] {
        let mut code = Source::Dword(0).instruction(0x4000);
        code.extend([0xda, 0x0d, 4, 0x40, 0, 0]); // FIMUL m32
        if negate {
            code.extend([0xd9, 0xe0]); // FCHS
        }
        code.extend([0xdb, 0x3d, 16, 0x40, 0, 0]); // FSTP m80
        let mut image = initial_image(&code, Source::Dword(0), 0xffff);
        image.data(0x8004, &(-1_i32).to_le_bytes());
        let module = TestModule::new(
            &Compiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&image.cpu)
                .compile_block(0x1000, &code, 3 + u32::from(negate))
                .unwrap(),
        );
        let mut result = Source::Dword(0).completed_load(image.cpu, 0x4000, 6);
        result = complete_x87(result, 6, 0x020d);
        if negate {
            result = complete_x87(result, 2, 0x01e0);
        }
        result = complete_x87(result, 6, 0x033d);
        result.x87.status.c1 = 0;
        result.x87.data_offset = 0x4010;
        let extended = (0, if negate { 0 } else { 0x8000 });
        write_register_bits(&mut result, 7, extended);
        assert_eq!(
            module.observe(&image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: result,
                    ram: &[(0x8010, &real80(extended))],
                    exit: Exit::Dispatch(result.eip),
                }]
            ),
            "FCHS {negate}"
        );
    }
}
