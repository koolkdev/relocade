use crate::{fixture::Fixture, wasm::Value};
use relocade_compiler::{RoundingInput, Type, I1, I32, I64};

#[test]
fn rounding_results_can_be_used_independently_or_reordered() {
    for order in [vec![0, 1, 2], vec![2, 0, 1, 2], vec![0], vec![1], vec![2]] {
        let returned = order.clone();
        let types = [Type::I64, Type::I1, Type::I1];
        let module = Fixture::new().function(
            &[
                Type::I64,
                Type::I32,
                Type::I32,
                Type::I32,
                Type::I32,
                Type::I32,
            ],
            &order
                .iter()
                .map(|&component| types[component])
                .collect::<Vec<_>>(),
            move |body| {
                let input = RoundingInput {
                    integer: body.parameter::<I64>(0)?,
                    guard: body.parameter::<I32>(2)?.truncate::<I1>(),
                    sticky: body.parameter::<I32>(3)?.truncate::<I1>(),
                };
                let rounded = input.round(
                    body.parameter::<I32>(1)?,
                    body.parameter::<I32>(4)?.truncate::<I1>(),
                    body.parameter::<I32>(5)?.truncate::<I1>(),
                );
                let results: [relocade_compiler::Argument; 3] = [
                    rounded.integer.into(),
                    rounded.inexact.into(),
                    rounded.incremented.into(),
                ];
                body.return_(
                    returned
                        .iter()
                        .map(|&component| results[component].clone())
                        .collect::<Vec<relocade_compiler::Argument>>(),
                )
            },
        );
        let mut instance = module.instantiate();
        for integer in [0, 3, 1 << 63, u64::MAX] {
            for distance in [0, 1, 63, 64, 65, u32::MAX] {
                for fraction in 0..4 {
                    let numerator = u128::from(integer) * 4 + fraction;
                    let denominator = 1_u128 << distance.saturating_add(2).min(127);
                    let floor = numerator / denominator;
                    let remainder = numerator % denominator;
                    for (nearest, away) in
                        [(false, false), (false, true), (true, false), (true, true)]
                    {
                        let incremented = if nearest {
                            remainder * 2 > denominator
                                || (remainder * 2 == denominator && floor & 1 != 0)
                        } else {
                            away && remainder != 0
                        };
                        let expected = [
                            Value::I64((floor + u128::from(incremented)) as i64),
                            Value::I32(i32::from(remainder != 0)),
                            Value::I32(i32::from(incremented)),
                        ];
                        // Narrowing a value retains unrelated upper carrier bits.
                        let actual = instance
                            .call_values(
                                "run",
                                &[
                                    Value::I64(integer as i64),
                                    Value::I32(distance as i32),
                                    Value::I32((fraction >> 1) as i32 | 0xfe),
                                    Value::I32((fraction & 1) as i32 | 0xfe),
                                    Value::I32(i32::from(nearest) | 0xfe),
                                    Value::I32(i32::from(away) | 0xfe),
                                ],
                            )
                            .unwrap();
                        assert_eq!(actual, order.iter().map(|&component| expected[component].clone()).collect::<Vec<_>>(),
                            "integer={integer:x}, distance={distance}, fraction={fraction}, nearest={nearest}, away={away}, order={order:?}");
                    }
                }
            }
        }
    }
}
