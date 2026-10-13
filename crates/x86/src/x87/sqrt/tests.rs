//! Exact integer square roots independently check the native estimate and refinement.

use super::*;
use crate::{test_step as step, CompiledModule};
use wasm86_compiler::{Program, Signature, Type, I1, I64};

#[test]
fn refined_root_and_rounding_evidence_match_integer_square_root() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64, Type::I1],
                results: vec![Type::I64, Type::I1, Type::I1],
            },
            |mut body| {
                let significand = body.parameter::<I64>(0)?;
                let odd = body.parameter::<I1>(1)?;
                let result = significand::calculate(&mut body, &significand, &odd)?;
                body.return_((result.integer, result.guard, result.sticky))
            },
        )
        .unwrap();
    program.export("sqrt", function).unwrap();
    let module = step::TestModule::new(&CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "sqrt".into(),
        execution_profile: None,
    });
    let mut values = vec![
        1 << 63,
        (1 << 63) + 1,
        (1 << 63) + 2047,
        0x9000_0000_0000_0000,
        0xdcce_60bd_0e0a_87ff, // The truncated native input needs an upward seed correction.
        u64::MAX - 2047,
        u64::MAX - 1,
        u64::MAX,
    ];
    let mut sample = 0x6288_40e5_d42b_7081_u64;
    for _ in 0..512 {
        sample ^= sample << 13;
        sample ^= sample >> 7;
        sample ^= sample << 17;
        values.push(sample | (1 << 63));
    }
    for significand in values {
        for odd in 0..2 {
            let radicand = u128::from(significand) << (63 + odd);
            let integer = radicand.isqrt();
            let remainder = radicand - integer * integer;
            let input = step::Input {
                arguments: vec![
                    step::Argument::I64(significand as i64),
                    step::Argument::I32(odd),
                ],
                ..step::Input::new(&[])
            };
            assert_eq!(
                module.observe(&input, 1).events,
                vec![step::Event::Return {
                    outcome: step::Outcome::Returned(vec![
                        step::Argument::I64(integer as i64),
                        step::Argument::I32(i32::from(remainder > integer)),
                        step::Argument::I32(i32::from(remainder != 0)),
                    ]),
                    snapshot: step::Snapshot {
                        cpu: vec![],
                        guest: None
                    },
                }],
                "significand={significand:x}, odd={odd}"
            );
        }
    }
}
