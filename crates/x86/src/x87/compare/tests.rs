use super::*;
use crate::x87::BinaryFormat;
use crate::{test_step as step, CompiledModule};
use wasm86_compiler::{Program, Signature, Type, I32, I64};

fn loaded_binary64_comparison(engine: step::Engine) {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64],
                results: vec![Type::I32, Type::I64],
            },
            |body| {
                let source = BinaryFormat::Binary64.decode(&body.parameter::<I64>(0)?);
                let value = source.loaded_value();
                let minimum_normal = BinaryFormat::Binary64
                    .decode(&0x0010_0000_0000_0000_u64.into())
                    .loaded_value();
                let compared =
                    BinaryOperands::new(&value, &minimum_normal).compare(ComparisonKind::Unordered);
                // Ordering predicates have no contract for unordered operands.
                let ordered = compared.unordered.eq(false);
                let comparison = [
                    ordered.and(compared.less),
                    ordered.and(compared.equal),
                    compared.unordered,
                    compared.invalid,
                    compared.denormal,
                ]
                .into_iter()
                .enumerate()
                .fold(Val::<I32>::from(0), |bits, (index, flag)| {
                    bits.or(flag.unsigned().extend::<I32>().shl(index as u32))
                });
                body.return_((comparison, value.binary64_value().unwrap().to_bits()))
            },
        )
        .unwrap();
    program.export("classify", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "classify".into(),
        execution_profile: None,
    });
    // Loaded subnormals are normal extended values; loaded SNaNs are quiet.
    // Comparison bits are LT/EQ/UN/IE/DE.
    for (bits, comparison, loaded_bits) in [
        (1_u64, 1, 1_u64),
        (0x000f_ffff_ffff_ffff, 1, 0x000f_ffff_ffff_ffff),
        (0x0010_0000_0000_0000, 2, 0x0010_0000_0000_0000),
        (0, 1, 0),
        (0x8000_0000_0000_0000, 1, 0x8000_0000_0000_0000),
        (0x7ff0_0000_0000_0000, 0, 0x7ff0_0000_0000_0000),
        (0x7ff8_0000_0000_0001, 4, 0x7ff8_0000_0000_0001),
        (0x7ff0_0000_0000_0001, 4, 0x7ff8_0000_0000_0001),
    ] {
        let input = step::Input {
            arguments: vec![step::Argument::I64(bits as i64)],
            ..step::Input::new(&[])
        };
        let expected = vec![step::Event::Return {
            outcome: step::Outcome::Returned(vec![
                step::Argument::I32(comparison),
                step::Argument::I64(loaded_bits as i64),
            ]),
            snapshot: step::Snapshot {
                cpu: vec![],
                guest: None,
            },
        }];
        assert_eq!(engine.observe(&module, &input, 1).events, expected);
    }
}

#[test]
fn loaded_binary64_values_compare_without_source_exceptions() {
    loaded_binary64_comparison(step::Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_loaded_binary64_values_compare_without_source_exceptions() {
    loaded_binary64_comparison(step::Engine::V8);
}
