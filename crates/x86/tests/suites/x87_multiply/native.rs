//! PC53/nearest products retain x87 rounding evidence and restart boundaries.

#[path = "native/boundaries.rs"]
mod boundaries;
#[path = "native/range.rs"]
mod range;

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::{Compiler, CpuState};
use wasmparser::{Operator, Parser, Payload};

fn assert_native_multiply(module: &TestModule) {
    assert!(Parser::new(0).parse_all(module.bytes()).any(|payload| {
        matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if
            body.get_operators_reader().unwrap().into_iter().any(|op| matches!(op.unwrap(), Operator::F64Mul)))
    }), "the fixture must reach native multiplication");
}

fn compiler() -> Compiler {
    let mut observed = CpuState::default();
    observed.x87.control.precision_control = 2;
    Compiler::new(SegmentProfile::Flat32).specialize_on_cpu(&observed)
}

fn rounding_and_forms(engine: Engine) {
    for (operation, destination, pop) in [
        ([0xd8, 0xc9], 7, false),
        ([0xdc, 0xc9], 0, false),
        ([0xde, 0xc9], 0, true),
    ] {
        let mut code = vec![0xdd, 0x05, 0, 0x40, 0, 0, 0xdd, 0x05, 8, 0x40, 0, 0];
        code.extend(operation);
        let module = TestModule::new(&compiler().compile_block(0x1000, &code, 3).unwrap());
        assert_native_multiply(&module);
        for (left, right, product, pe, c1) in [
            (LEADING, LEADING, (LEADING, 0x3fff), 0, 0),
            (
                LEADING + 0x800,
                LEADING + 0x800,
                (LEADING + 0x1000, 0x3fff),
                1,
                0,
            ),
            // Halfway cases round to an even low bit in either direction.
            (
                LEADING + 0x800,
                0xc000_0000_0000_0000,
                (0xc000_0000_0000_1000, 0x3fff),
                1,
                1,
            ),
            (
                LEADING + 0x1800,
                0xc000_0000_0000_0000,
                (0xc000_0000_0000_2000, 0x3fff),
                1,
                0,
            ),
            // The exact product is 2 - 2^-103; rounding crosses a binade.
            (
                LEADING + 0x800,
                0xffff_ffff_ffff_f000,
                (LEADING, 0x4000),
                1,
                1,
            ),
        ] {
            for negative in [false, true] {
                let sign = if negative { 0x8000 } else { 0 };
                let mut image = stack_image(&code, 1, 0xffff);
                set_control(&mut image.cpu.x87.control, 0x027f);
                image.cpu.x87.status.precision = 0;
                image.map(4, 0x8000, true);
                let binary64 = |m: u64, sign: u16| {
                    0x3ff0_0000_0000_0000 | ((m & (LEADING - 1)) >> 11) | (u64::from(sign) << 48)
                };
                image.data(0x8000, &binary64(right, 0).to_le_bytes());
                image.data(0x8008, &binary64(left, sign).to_le_bytes());
                let mut result = complete_x87(image.cpu, 6, 0x0505);
                result = complete_x87(result, 6, 0x0505);
                result.x87.status.top = 7;
                result.x87.data_offset = 0x4008;
                result.x87.data_selector = 0x23;
                write_value(&mut result, 0, (right, 0x3fff));
                write_value(&mut result, 7, (left, 0x3fff | sign));
                result = complete_x87(result, 2, (u16::from(operation[0] & 7) << 8) | 0xc9);
                result.x87.status.precision = pe;
                result.x87.status.c1 = c1;
                write_value(&mut result, destination, (product.0, product.1 | sign));
                if pop {
                    result.x87.status.top = 0;
                    result.x87.tag_word |= 0xc000;
                }
                assert_eq!(
                    engine.observe(&module, &image.input(), 1),
                    expected(&image, &[dispatch(result)])
                );
            }
        }
    }
}

fn signed_zeros(engine: Engine) {
    let code = [
        0xdd, 0x05, 0, 0x40, 0, 0, 0xdd, 0x05, 8, 0x40, 0, 0, 0xd8, 0xc9,
    ];
    let module = TestModule::new(&compiler().compile_block(0x1000, &code, 3).unwrap());
    assert_native_multiply(&module);
    for (left, right) in [
        ((0, 0), (LEADING, 0x3fff)),
        ((LEADING, 0x3fff), (0, 0)),
        ((0, 0), (0, 0)),
    ] {
        for left_sign in [0, 0x8000] {
            for right_sign in [0, 0x8000] {
                let mut image = stack_image(&code, 1, 0xffff);
                set_control(&mut image.cpu.x87.control, 0x027f);
                image.map(4, 0x8000, true);
                let binary64 = |m, sign: u16| {
                    (if m == 0 { 0 } else { 0x3ff0_0000_0000_0000_u64 }) | (u64::from(sign) << 48)
                };
                image.data(0x8000, &binary64(right.0, right_sign).to_le_bytes());
                image.data(0x8008, &binary64(left.0, left_sign).to_le_bytes());
                let mut result = complete_x87(image.cpu, 6, 0x0505);
                result = complete_x87(result, 6, 0x0505);
                result = complete_x87(result, 2, 0x00c9);
                result.x87.status.top = 7;
                result.x87.status.c1 = 0;
                result.x87.data_offset = 0x4008;
                result.x87.data_selector = 0x23;
                write_value(&mut result, 0, (right.0, right.1 | right_sign));
                write_value(&mut result, 7, (0, left_sign ^ right_sign));
                assert_eq!(
                    engine.observe(&module, &image.input(), 1),
                    expected(&image, &[dispatch(result)])
                );
            }
        }
    }
}

fn live_load_multiply_store(engine: Engine) {
    for (load, memory_mul, input) in [
        (0xd9, 0xd8, vec![0, 0, 0xc0, 0x3f]), // binary32: 1.5
        (0xdd, 0xdc, 0x3ff8_0000_0000_0000_u64.to_le_bytes().to_vec()),
    ] {
        let code = [
            load, 0x05, 0, 0x40, 0, 0, // FLD 1.5
            memory_mul, 0x0d, 0, 0x40, 0, 0, // FMUL 1.5
            0xd8, 0xc8, // FMUL ST0, ST0
            0xdd, 0x1d, 8, 0x40, 0, 0, // FSTP m64
        ];
        let module = TestModule::new(&compiler().compile_block(0x1000, &code, 4).unwrap());
        let mut image = stack_image(&code, 0, 0xffff);
        set_control(&mut image.cpu.x87.control, 0x027f);
        image.map(4, 0x8000, true);
        image.data(0x8000, &input);
        let mut result = complete_x87(image.cpu, 6, (u16::from(load & 7) << 8) | 5);
        result = complete_x87(result, 6, (u16::from(memory_mul & 7) << 8) | 0x0d);
        result = complete_x87(result, 2, 0x00c8);
        result = complete_x87(result, 6, 0x051d);
        result.x87.data_offset = 0x4008;
        result.x87.data_selector = 0x23;
        result.x87.status.c1 = 0;
        // Popping empties the slot but retains its final 5.0625 payload.
        write_register_bits(&mut result, 7, (0xa200_0000_0000_0000, 0x4001));
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: result,
                    ram: &[(0x8008, &0x4014_4000_0000_0000_u64.to_le_bytes())],
                    exit: Exit::Dispatch(result.eip),
                }]
            )
        );
    }
}

#[test]
fn binary64_products_preserve_rounding_forms_and_live_values() {
    rounding_and_forms(Engine::Wasmtime);
    signed_zeros(Engine::Wasmtime);
    live_load_multiply_store(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_binary64_products_preserve_rounding_forms_and_live_values() {
    rounding_and_forms(Engine::V8);
    signed_zeros(Engine::V8);
    live_load_multiply_store(Engine::V8);
}
