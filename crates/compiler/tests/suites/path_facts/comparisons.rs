//! Opposite comparisons share facts within the branch that proved them.
use super::*;
use wasm86_compiler::I64;

fn constant_comparison(v8: bool) {
    let module = Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        body.if_(input.ne(32767), |arm| {
            arm.return_(input.eq(32767).select::<I32>(7, 11))
        })?;
        body.return_(13)
    });
    assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 0);
    for (input, expected) in [(0, 11), (32767, 13), (-1, 11)] {
        check_result(&module, &[Value::I32(input)], &[Value::I32(expected)], v8);
    }
}

fn opposite_comparisons(v8: bool) {
    for kind in 0..6 {
        let module = Fixture::new().function(&[Type::I64; 2], &[Type::I32], |mut body| {
            let left = body.parameter::<I64>(0)?;
            let right = body.parameter::<I64>(1)?;
            let opposite = match kind {
                0 => right.ne(&left),
                1 => right.eq(&left),
                2 => left.unsigned().ge(&right),
                3 => left.unsigned().lt(&right),
                4 => left.signed().ge(&right),
                _ => left.signed().lt(&right),
            };
            let guard = match kind {
                0 => left.eq(&right),
                1 => left.ne(&right),
                2 => left.unsigned().lt(&right),
                3 => left.unsigned().ge(&right),
                4 => left.signed().lt(&right),
                _ => left.signed().ge(&right),
            };
            body.if_(guard, |arm| arm.return_(opposite.select::<I32>(7, 11)))?;
            body.return_(opposite.select::<I32>(13, 17))
        });
        assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 0);
        for (left, right) in [
            (0, 0),
            (7, 7),
            (7, 9),
            (9, 7),
            (-1, -1),
            (-1, 0),
            (i64::MIN, i64::MAX),
            (i64::MAX, i64::MIN),
            (0x1_0000_0000, 0),
        ] {
            let truth = match kind {
                0 => left == right,
                1 => left != right,
                2 => (left as u64) < right as u64,
                3 => (left as u64) >= right as u64,
                4 => left < right,
                _ => left >= right,
            };
            check_result(
                &module,
                &[Value::I64(left), Value::I64(right)],
                &[Value::I32(if truth { 11 } else { 13 })],
                v8,
            );
        }
    }
}

fn comparison_boundaries(v8: bool) {
    let module = Fixture::new().function(
        &[Type::I32, Type::I32, Type::I1],
        &[Type::I32; 2],
        |mut body| {
            let left = body.parameter::<I32>(0)?;
            let right = body.parameter::<I32>(1)?;
            let enabled = body.parameter::<I1>(2)?;
            body.if_(enabled, |mut arm| {
                arm.if_(left.truncate::<I8>().ne(right.truncate::<I8>()), |exit| {
                    exit.return_((7, 7))
                })?;
                arm.if_(left.signed().lt(&right), |exit| {
                    exit.return_((
                        left.ne(&right).select::<I32>(11, 13),
                        left.unsigned().ge(&right).select::<I32>(17, 19),
                    ))
                })
            })?;
            body.return_((
                left.eq(&right).select::<I32>(23, 29),
                left.signed().ge(&right).select::<I32>(31, 37),
            ))
        },
    );
    for (left, right) in [(7, 7), (7, 9), (0, 256), (-1, 255), (255, -1)] {
        for enabled in [0, 1] {
            let expected = if enabled != 0 && (left as u8) != right as u8 {
                [7, 7]
            } else if enabled != 0 && left < right {
                [
                    11,
                    if (left as u32) >= right as u32 {
                        17
                    } else {
                        19
                    },
                ]
            } else {
                [
                    if left == right { 23 } else { 29 },
                    if left >= right { 31 } else { 37 },
                ]
            };
            check_result(
                &module,
                &[Value::I32(left), Value::I32(right), Value::I32(enabled)],
                &expected.map(Value::I32),
                v8,
            );
        }
    }
}

#[test]
fn inequality_guard_eliminates_the_opposite_equality() {
    constant_comparison(false);
}

#[test]
fn both_edges_prove_opposite_comparisons_including_swapped_equality() {
    opposite_comparisons(false);
}

#[test]
fn comparison_facts_respect_width_signedness_and_bypassed_guards() {
    comparison_boundaries(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_comparison_facts_preserve_results_and_boundaries() {
    constant_comparison(true);
    opposite_comparisons(true);
    comparison_boundaries(true);
}
