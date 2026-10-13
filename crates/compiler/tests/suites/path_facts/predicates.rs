use super::*;
use wasm86_compiler::I64;

fn excluded_interval() -> TestModule {
    Fixture::new().function(&[Type::I64], &[Type::I64], |mut body| {
        let input = body.parameter::<I64>(0)?;
        let excluded = input.ne(0_u64).and(input.unsigned().lt(8_u64));
        body.if_(&excluded, |arm| arm.return_(1_u64))?;
        let retained = excluded.select(0_u64, input);
        body.if_(retained.eq(0_u64), |arm| arm.return_(0_u64))?;
        body.return_(retained.unsigned().lt(8_u64).select(retained.clz(), 42_u64))
    })
}

fn check_interval(v8: bool) {
    let module = excluded_interval();
    assert_eq!(count(&module, |op| matches!(op, Operator::I64Clz)), 0);
    for (input, expected) in [
        (0, 0),
        (1, 1),
        (7, 1),
        (8, 42),
        (9, 42),
        (i64::MAX, 42),
        (-1, 42),
    ] {
        check_result(&module, &[Value::I64(input)], &[Value::I64(expected)], v8);
    }
}

#[test]
fn a_later_operand_fact_removes_an_excluded_fallback_after_rewriting() {
    check_interval(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_a_later_operand_fact_removes_an_excluded_fallback_after_rewriting() {
    check_interval(true);
}
