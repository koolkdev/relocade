//! Sign changes retain native PC53 arithmetic and exact binary load/store paths.

use super::*;
use crate::support::{machine::expected, step::TestModule, x87::write_register_bits};
use wasm86_x86::{CompiledModule, Compiler, CpuState};
use wasmparser::{Operator, Parser, Payload};

fn compiler() -> Compiler {
    let mut observed = CpuState::default();
    set_control(&mut observed.x87.control, 0x027f);
    Compiler::new(SegmentProfile::Flat32).specialize_on_cpu(&observed)
}

fn native_operations(module: &CompiledModule) -> Vec<&'static str> {
    Parser::new(0)
        .parse_all(&module.bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(body.get_operators_reader().unwrap()),
            _ => None,
        })
        .flat_map(|reader| reader.into_iter().map(Result::unwrap))
        .filter_map(|operator| match operator {
            Operator::F64Add => Some("add"),
            Operator::F64Mul => Some("multiply"),
            Operator::F64Div => Some("divide"),
            _ => None,
        })
        .collect()
}

#[test]
fn repeated_pc53_sign_operations_fold_before_emission() {
    for (signs, control) in [([0xe0, 0xe0], [0xc8, 0xc8]), ([0xe1, 0xe1], [0xe1, 0xc8])] {
        let code = |operations: [u8; 2]| {
            [
                0xdd,
                0x05,
                0,
                0x40,
                0,
                0, // FLD m64
                0xdc,
                0x0d,
                8,
                0x40,
                0,
                0, // FMUL m64 establishes PC53
                0xd9,
                operations[0],
                0xd9,
                operations[1],
                0xdd,
                0xd0, // FST ST0 gives both blocks the same final opcode.
            ]
        };
        let actual = compiler().compile_block(0x1000, &code(signs), 5).unwrap();
        let expected = compiler().compile_block(0x1000, &code(control), 5).unwrap();
        assert_eq!(actual.bytes, expected.bytes);
    }
}

fn pc53_arithmetic(engine: Engine) {
    for (consumer, operation, positive_result, negative_result) in [
        (
            0x05,
            "add",
            (0xa000_0000_0000_0000, 0x4001),
            (0x8000_0000_0000_0000, 0xbfff),
        ),
        (
            0x0d,
            "multiply",
            (0xc000_0000_0000_0000, 0x4001),
            (0xc000_0000_0000_0000, 0xc001),
        ),
        (
            0x35,
            "divide",
            (0xc000_0000_0000_0000, 0x3fff),
            (0xc000_0000_0000_0000, 0xbfff),
        ),
    ] {
        for sign_opcode in [0xe0, 0xe1] {
            let code = [
                0xdd,
                0x05,
                0,
                0x40,
                0,
                0, // FLD m64
                0xdc,
                0x0d,
                8,
                0x40,
                0,
                0, // FMUL m64: produces native PC53
                0xd9,
                sign_opcode,
                0xdc,
                consumer,
                8,
                0x40,
                0,
                0,
            ];
            let compiled = compiler().compile_block(0x1000, &code, 4).unwrap();
            let mut control = code;
            control[13] = 0xc8; // FXCH ST0 keeps the same value representation.
            let control = compiler().compile_block(0x1000, &control, 4).unwrap();
            let operations = native_operations(&compiled);
            assert!(operations.contains(&operation));
            assert_eq!(operations, native_operations(&control));
            let module = TestModule::new(&compiled);
            for input in [1.5_f64, -1.5, 0.0, -0.0] {
                let mut image = stack_image(&code, 0, 0xffff);
                set_control(&mut image.cpu.x87.control, 0x027f);
                image.cpu.x87.status.precision = 0;
                image.map(4, 0x8000, false);
                image.data(0x8000, &input.to_le_bytes());
                image.data(0x8008, &0x4000_0000_0000_0000_u64.to_le_bytes());
                let mut result = complete_x87(image.cpu, 6, 0x0505);
                result = complete_x87(result, 6, 0x040d);
                result = complete_x87(result, 2, 0x0100 | u16::from(sign_opcode));
                result = complete_x87(result, 6, 0x0400 | u16::from(consumer));
                result.x87.status.top = 7;
                result.x87.status.c1 = 0;
                result.x87.data_offset = 0x4008;
                result.x87.data_selector = 0x23;
                // ±1.5 * 2 becomes ±3; the consumer then adds, multiplies or
                // divides by 2. These exact results need no rounding response.
                let negative = sign_opcode == 0xe0 && !input.is_sign_negative();
                let value = if input == 0.0 && consumer == 0x05 {
                    (0x8000_0000_0000_0000, 0x4000) // Either signed zero plus two.
                } else if input == 0.0 {
                    (0, if negative { 0x8000 } else { 0 })
                } else if negative {
                    negative_result
                } else {
                    positive_result
                };
                write_value(&mut result, 7, value);
                assert_eq!(
                    engine.observe(&module, &image.input(), 1),
                    expected(&image, &[dispatch(result)]),
                    "{operation}, sign opcode={sign_opcode:x}, input={input}"
                );
            }
        }
    }
}

fn pc53_extended_range(engine: Engine) {
    for opcode in [0xe0, 0xe1] {
        let code = [
            0xdc, 0x0d, 0, 0x40, 0, 0, // FMUL m64 by one establishes PC53
            0xd9, opcode, 0xdc, 0x35, 8, 0x40, 0, 0, // FDIV m64 by two
        ];
        let compiled = compiler().compile_block(0x1000, &code, 3).unwrap();
        assert!(native_operations(&compiled).contains(&"divide"));
        let module = TestModule::new(&compiled);
        for exponent in [3, 0x43ff, 0x7ffe] {
            let mut image = stack_image(&code, 0, 0xfffc);
            set_control(&mut image.cpu.x87.control, 0x027f);
            image.cpu.x87.status.precision = 0;
            write_value(
                &mut image.cpu,
                0,
                (0xc000_0000_0000_0000, exponent | 0x8000),
            );
            image.map(4, 0x8000, false);
            image.data(0x8000, &0x3ff0_0000_0000_0000_u64.to_le_bytes());
            image.data(0x8008, &0x4000_0000_0000_0000_u64.to_le_bytes());
            let mut result = complete_x87(image.cpu, 6, 0x040d);
            result = complete_x87(result, 2, 0x0100 | u16::from(opcode));
            result = complete_x87(result, 6, 0x0435);
            result.x87.status.c1 = 0;
            result.x87.data_offset = 0x4008;
            result.x87.data_selector = 0x23;
            write_value(&mut result, 0, (0xc000_0000_0000_0000, exponent - 1));
            assert_eq!(
                engine.observe(&module, &image.input(), 1),
                expected(&image, &[dispatch(result)]),
                "sign opcode={opcode:x}, exponent={exponent:x}"
            );
        }
    }
}

fn binary_load_store(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (load, input, extended) in [
        (0xd9, 0xbf80_0000_u64, (0x8000_0000_0000_0000, 0x3fff)),
        (0xdd, 0x8000_0000_0000_0000, (0, 0)),
        (0xdd, 0xfff8_0000_0000_0042, (0xc000_0000_0002_1000, 0x7fff)),
    ] {
        for opcode in [0xe0, 0xe1] {
            let code = [
                load, 0x05, 0, 0x40, 0, 0, 0xd9, opcode, load, 0x1d, 8, 0x40, 0, 0,
            ];
            let mut image = stack_image(&code, 0, 0xffff);
            image.map(4, 0x8000, true);
            image.data(0x8000, &input.to_le_bytes());
            let mut loaded = complete_x87(image.cpu, 6, (u16::from(load & 7) << 8) | 5);
            loaded.x87.status.top = 7;
            loaded.x87.status.c1 = 0;
            loaded.x87.data_offset = 0x4000;
            loaded.x87.data_selector = 0x23;
            write_value(&mut loaded, 7, (extended.0, extended.1 | 0x8000));
            let mut changed = complete_x87(loaded, 2, 0x0100 | u16::from(opcode));
            write_register_bits(&mut changed, 7, extended);
            let mut stored = complete_x87(changed, 6, (u16::from(load & 7) << 8) | 0x1d);
            stored.x87.status.top = 0;
            stored.x87.tag_word = 0xffff;
            stored.x87.data_offset = 0x4008;
            let (bytes, sign_bit) = if load == 0xd9 {
                (4, 1 << 31)
            } else {
                (8, 1 << 63)
            };
            let output = (input & !sign_bit).to_le_bytes();
            checks.check(
                "binary load retains signed zero and NaN payload through sign and store",
                &code,
                &image,
                &[
                    dispatch(loaded),
                    dispatch(changed),
                    Step {
                        cpu: stored,
                        ram: &[(0x8008, &output[..bytes])],
                        exit: Exit::Dispatch(stored.eip),
                    },
                ],
            );
        }
    }
}

#[test]
fn sign_changes_retain_native_pc53_arithmetic() {
    pc53_arithmetic(Engine::Wasmtime);
    pc53_extended_range(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_sign_changes_retain_native_pc53_arithmetic() {
    pc53_arithmetic(Engine::V8);
    pc53_extended_range(Engine::V8);
}

test_frontends!(binary_round_trips, binary_load_store);
