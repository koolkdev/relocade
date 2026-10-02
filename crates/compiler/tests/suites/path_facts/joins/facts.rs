//! A join retains only knowledge established by every continuing path.
use super::*;

fn common_bits() -> TestModule {
    Fixture::new().function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let mode = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?;
        body.if_else(
            mode,
            |mut arm| arm.if_(input.and(3).ne(1), |exit| exit.return_(7)),
            |mut arm| arm.if_(input.and(3).ne(3), |exit| exit.return_(9)),
        )?;
        body.return_(input.and(1).eq(1).select::<I32>(11, 13))
    })
}

fn common_ranges() -> TestModule {
    Fixture::new().function(&[Type::I1, Type::I32], &[Type::I32], |mut body| {
        let mode = body.parameter::<I1>(0)?;
        let input = body.parameter::<I32>(1)?;
        body.if_else(
            mode,
            |mut arm| arm.if_(input.unsigned().ge(10), |exit| exit.return_(7)),
            |mut arm| arm.if_(input.unsigned().ge(20), |exit| exit.return_(9)),
        )?;
        body.return_(
            input
                .unsigned()
                .lt(20)
                .select::<I32>(11, 13)
                .add(input.unsigned().lt(10).select::<I32>(100, 200)),
        )
    })
}

fn common_comparison() -> TestModule {
    Fixture::new().function(
        &[Type::I1, Type::I32, Type::I32],
        &[Type::I32],
        |mut body| {
            let mode = body.parameter::<I1>(0)?;
            let left = body.parameter::<I32>(1)?;
            let right = body.parameter::<I32>(2)?;
            body.if_else(
                mode,
                |mut arm| arm.if_(left.ne(&right), |exit| exit.return_(7)),
                |mut arm| arm.if_(right.ne(&left), |exit| exit.return_(9)),
            )?;
            body.return_(left.eq(right).select::<I32>(11, 13))
        },
    )
}

fn check_common_bits(v8: bool) {
    let module = common_bits();
    assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 0);
    for (mode, input, expected) in [(1, 1, 11), (0, 3, 11), (1, 3, 7), (0, 1, 9), (0, -1, 11)] {
        check_result(
            &module,
            &[Value::I32(mode), Value::I32(input)],
            &[Value::I32(expected)],
            v8,
        );
    }
}

fn check_common_ranges(v8: bool) {
    let module = common_ranges();
    assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 1);
    for (mode, input, expected) in [
        (1, 9, 111),
        (0, 9, 111),
        (1, 10, 7),
        (0, 10, 211),
        (0, 19, 211),
        (0, 20, 9),
        (1, -1, 7),
    ] {
        check_result(
            &module,
            &[Value::I32(mode), Value::I32(input)],
            &[Value::I32(expected)],
            v8,
        );
    }
}

fn check_common_comparison(v8: bool) {
    let module = common_comparison();
    assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 0);
    for (mode, left, right, expected) in
        [(1, 3, 3, 11), (0, -1, -1, 11), (1, 3, 4, 7), (0, 3, 4, 9)]
    {
        check_result(
            &module,
            &[Value::I32(mode), Value::I32(left), Value::I32(right)],
            &[Value::I32(expected)],
            v8,
        );
    }
}

#[test]
fn joined_bit_facts_keep_only_agreeing_bits() {
    check_common_bits(false);
}

#[test]
fn joined_ranges_cover_both_incoming_intervals() {
    check_common_ranges(false);
}

#[test]
fn joined_comparisons_preserve_common_outcomes() {
    check_common_comparison(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_common_join_facts_preserve_results() {
    check_common_bits(true);
    check_common_ranges(true);
    check_common_comparison(true);
}
