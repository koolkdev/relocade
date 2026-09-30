use super::*;
use crate::{test_step as step, CompiledModule};
use wasm86_compiler::{Program, Signature, Type};

#[test]
fn integer_rounding_preserves_evidence_across_word_boundaries() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I32, Type::I8, Type::I1],
                results: vec![Type::I64, Type::I1, Type::I1],
            },
            |body| {
                let input = RoundingInput::shift_right(
                    &body.parameter::<I64>(0)?,
                    body.parameter::<I32>(1)?,
                );
                let mode = RoundingMode::new(body.parameter::<I8>(2)?);
                let result = mode.round(input, &body.parameter::<I1>(3)?);
                body.return_((result.integer, result.inexact, result.incremented))
            },
        )
        .unwrap();
    program.export("round", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "round".into(),
        segment_profile: None,
    });
    for value in [0, 1, 3, (1 << 63) - 1, 1 << 63, (1 << 63) + 1, u64::MAX] {
        for distance in [0, 1, 2, 31, 32, 63, 64, 65, 128, 16384] {
            // Rational division is independent of the emitted bit extraction.
            // Beyond 127 every u64 is strictly below the midpoint with floor 0.
            let denominator = 2_u128.pow(distance.min(127));
            let numerator = u128::from(value);
            let quotient = numerator / denominator;
            let remainder = numerator % denominator;
            for mode in 0..4 {
                for negative in [false, true] {
                    let increment = match mode {
                        0 => {
                            remainder * 2 > denominator
                                || (remainder * 2 == denominator && quotient % 2 == 1)
                        }
                        1 => remainder != 0 && negative,
                        2 => remainder != 0 && !negative,
                        _ => false,
                    };
                    let input = step::Input {
                        arguments: vec![
                            step::Argument::I64(value as i64),
                            step::Argument::I32(distance as i32),
                            step::Argument::I32(mode | 0xfc),
                            step::Argument::I32(i32::from(negative)),
                        ],
                        ..step::Input::new(&[])
                    };
                    assert_eq!(
                        module.observe(&input, 1).events,
                        vec![step::Event::Return {
                            outcome: step::Outcome::Returned(vec![
                                step::Argument::I64((quotient + u128::from(increment)) as i64),
                                step::Argument::I32(i32::from(remainder != 0)),
                                step::Argument::I32(i32::from(increment))
                            ]),
                            snapshot: step::Snapshot {
                                cpu: vec![],
                                guest: None
                            },
                        }],
                        "{value:x} / 2^{distance}, mode {mode}, negative {negative}"
                    );
                }
            }
        }
    }
}
