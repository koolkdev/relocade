//! Denominator precision changes the algorithm without narrowing the accumulator.

use super::*;
use crate::support::{machine::expected, step::TestModule};
use wasm86_x86::compile_block_from_bytes;
use wasmparser::{Operator, Parser, Payload};

fn memory_quotients(engine: Engine, frontend: Frontend) {
    let code = [0xdc, 0x35, 0, 0x40, 0, 0]; // FDIV m64
    let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap());
    assert!(Parser::new(0).parse_all(block.bytes()).any(|payload| {
        matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if body.get_operators_reader().unwrap()
            .into_iter().any(|op| matches!(op.unwrap(), Operator::F64Div)))
    }), "the fixture must use a native quotient estimate");
    let module = match frontend {
        Frontend::Block => &block,
        Frontend::Interpreter => TestModule::interpreter(),
    };
    let mut pairs = vec![
        // Estimates requiring a decrement, no repair, one increment and two
        // increments. The oracle below computes the exact rational directly.
        (0xedb1_cc15_50e6_68d6, 0xff9d_9471_fc51_6800),
        (0xefca_66cc_5f13_128a, 0xb273_9844_dc67_f800),
        (0xa5e5_61b7_2942_0504, 0xb98d_0dd5_ffc6_e000),
        (0x8ce0_5b3e_4377_afdf, 0x98d0_5c8a_57d6_1000),
    ];
    for denominator in [
        LEADING,
        LEADING + 2048,
        0xc000_0000_0000_0000,
        u64::MAX - 2047,
    ] {
        for numerator in [
            LEADING,
            LEADING + 1,
            denominator - 1,
            denominator,
            denominator + 1,
            u64::MAX,
        ] {
            if numerator >= LEADING {
                pairs.push((numerator, denominator));
            }
        }
    }
    // Exact midpoints after reducing precision, and the neighboring low bits.
    for tail in [
        (1 << 10) - 1,
        1 << 10,
        (1 << 10) + 1,
        3 << 10,
        1 << 39,
        3 << 39,
    ] {
        pairs.push((LEADING + tail, LEADING));
    }
    for (numerator, denominator) in pairs {
        for (pc, precision) in [(0, 24), (1, 64), (2, 53), (3, 64)] {
            for rc in 0..4 {
                for negative in [false, true] {
                    let mut image = stack_image(&code, 0, 0xfffc);
                    image.cpu.x87.status.precision = 0;
                    set_control(&mut image.cpu.x87.control, 0x007f | (pc << 8) | (rc << 10));
                    write_value(
                        &mut image.cpu,
                        0,
                        (numerator, 0x3fff | if negative { 0x8000 } else { 0 }),
                    );
                    image.map(4, 0x8000, false);
                    let source = 0x3ff0_0000_0000_0000 | ((denominator & (LEADING - 1)) >> 11);
                    image.data(0x8000, &source.to_le_bytes());
                    let (quotient, flags) =
                        rounding::exact_quotient(numerator, denominator, precision, rc, negative);
                    let mut result = complete_x87(image.cpu, 6, 0x0435);
                    result.x87.data_offset = 0x4000;
                    result.x87.data_selector = 0x23;
                    result.x87.status.precision = u8::from(flags & PE != 0);
                    result.x87.status.c1 = u8::from(flags & C1 != 0);
                    write_value(&mut result, 0, quotient);
                    assert_eq!(
                        engine.observe(module, &image.input(), 1),
                        expected(&image, &[dispatch(result)]),
                        "{numerator:x}/{denominator:x}, PC={pc}, RC={rc}, negative={negative}"
                    );
                }
            }
        }
    }
}

fn loaded_denominators(engine: Engine, frontend: Frontend) {
    // Loading only the numerator must not establish precision for a full-width
    // divisor. Reversal also makes a loaded destination the actual denominator.
    for (prefix, opcode, destination, loaded_is_divisor) in [
        (0xdc, 0xf9, 0, true),
        (0xd8, 0xf9, 7, true),
        (0xdc, 0xf1, 0, false),
    ] {
        let code = [0xdd, 0x05, 0, 0x40, 0, 0, prefix, opcode];
        let mut image = stack_image(&code, 0, 0xfffc);
        image.cpu.x87.status.precision = 0;
        let wide = LEADING + 1;
        let narrow = 0xb98d_0dd5_ffc6_e000;
        write_value(&mut image.cpu, 0, (wide, 0x3fff));
        image.map(4, 0x8000, false);
        image.data(
            0x8000,
            &(0x3ff0_0000_0000_0000 | ((narrow & (LEADING - 1)) >> 11)).to_le_bytes(),
        );
        let mut loaded = complete_x87(image.cpu, 6, 0x0505);
        loaded.x87.status.top = 7;
        loaded.x87.status.c1 = 0;
        loaded.x87.data_offset = 0x4000;
        loaded.x87.data_selector = 0x23;
        write_value(&mut loaded, 7, (narrow, 0x3fff));
        let (numerator, denominator) = if loaded_is_divisor {
            (wide, narrow)
        } else {
            (narrow, wide)
        };
        let (quotient, flags) = rounding::exact_quotient(numerator, denominator, 64, 0, false);
        let mut result = complete_x87(loaded, 2, (u16::from(prefix & 7) << 8) | u16::from(opcode));
        result.x87.status.precision = u8::from(flags & PE != 0);
        result.x87.status.c1 = u8::from(flags & C1 != 0);
        write_value(&mut result, destination, quotient);
        let block = TestModule::new(&compile_block_from_bytes(0x1000, &code, 2).unwrap());
        let native_estimate = Parser::new(0).parse_all(block.bytes()).any(|payload| {
            matches!(payload.unwrap(), Payload::CodeSectionEntry(body) if body.get_operators_reader().unwrap()
                .into_iter().any(|op| matches!(op.unwrap(), Operator::F64Div)))
        });
        assert_eq!(native_estimate, loaded_is_divisor);
        let (module, steps) = match frontend {
            Frontend::Block => (&block, vec![dispatch(result)]),
            Frontend::Interpreter => (
                TestModule::interpreter(),
                vec![dispatch(loaded), dispatch(result)],
            ),
        };
        assert_eq!(
            engine.observe(module, &image.input(), steps.len()),
            expected(&image, &steps),
            "division uses the actual denominator's precision"
        );
    }
}

test_frontends!(precision, memory_quotients);
test_frontends!(provenance, loaded_denominators);
