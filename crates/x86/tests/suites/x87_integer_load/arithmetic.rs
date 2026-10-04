//! Integer loads preserve exact inputs for the first arithmetic operation.

use super::*;
use wasm86_x86::BlockCompiler;
use wasmparser::{Operator, Parser, Payload};

fn integer_products(engine: Engine) {
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
            &BlockCompiler::new(SegmentProfile::Flat32)
                .specialize_on_cpu(&observed)
                .compile(0x1000, &code, 2)
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
                engine.observe(&module, &image.input(), 1),
                expected(&image, &[dispatch(result)]),
                "FILD {source:?}, negative factor {negative_factor}"
            );
        }
    }
}

#[test]
fn first_product_uses_exact_integer_load() {
    integer_products(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_first_product_uses_exact_integer_load() {
    integer_products(Engine::V8);
}
