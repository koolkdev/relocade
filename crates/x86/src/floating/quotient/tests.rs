//! Independent integer division checks native correction and wide quotient digits.

use super::*;
use crate::{test_step as step, CompiledModule};
use wasm86_compiler::{Program, Signature, Type};

fn check_scaled_division(narrow: bool) {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I64, Type::I32],
                results: vec![Type::I64, Type::I64],
            },
            |mut body| {
                let numerator = body.parameter::<I64>(0)?;
                let denominator = body.parameter::<I64>(1)?;
                let shift = body.parameter::<I32>(2)?;
                let result = integer_division(&mut body, &numerator, denominator, &shift, narrow)?;
                body.return_((result.quotient, result.remainder))
            },
        )
        .unwrap();
    program.export("divide", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "divide".into(),
        execution_target: None,
    });
    let boundaries = [
        LEADING,
        LEADING + 1,
        LEADING + 2047,
        LEADING + 2048,
        0xc000_0000_0000_0000,
        u64::MAX - 2047,
        u64::MAX - 1,
        u64::MAX,
    ];
    let mut cases = Vec::new();
    for numerator in boundaries {
        for denominator in boundaries {
            for shift in [0, 1, 31, 32, 33, 51, 52, 53, 62, 63] {
                cases.push((numerator, denominator, shift));
            }
        }
    }
    let mut sample = 0x51a4_6293_e875_bacf_u64;
    let mut next = || {
        sample ^= sample << 13;
        sample ^= sample >> 7;
        sample ^= sample << 17;
        sample
    };
    for _ in 0..512 {
        cases.push((next() | LEADING, next() | LEADING, (next() & 63) as u32));
    }
    for (numerator, denominator, shift) in cases {
        let denominator = if narrow {
            denominator & !2047
        } else {
            denominator
        };
        let dividend = u128::from(numerator) << shift;
        let quotient = (dividend / u128::from(denominator)) as u64;
        let remainder = (dividend % u128::from(denominator)) as u64;
        let input = step::Input {
            arguments: vec![
                step::Argument::I64(numerator as i64),
                step::Argument::I64(denominator as i64),
                step::Argument::I32(shift as i32),
            ],
            ..step::Input::new(&[])
        };
        assert_eq!(
            module.observe(&input, 1).events,
            vec![step::Event::Return {
                outcome: step::Outcome::Returned(vec![
                    step::Argument::I64(quotient as i64),
                    step::Argument::I64(remainder as i64),
                ]),
                snapshot: step::Snapshot {
                    cpu: vec![],
                    guest: None
                },
            }],
            "numerator={numerator:x} denominator={denominator:x} shift={shift} narrow={narrow}",
        );
    }
}

#[test]
fn full_width_division_matches_u128() {
    check_scaled_division(false);
}

#[test]
fn native_prefix_division_matches_u128() {
    check_scaled_division(true);
}
