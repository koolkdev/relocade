//! Branch facts simplify their own continuation without changing other paths.

#[path = "path_facts/bitwise.rs"]
mod bitwise;
#[path = "path_facts/boundaries.rs"]
mod boundaries;
#[path = "path_facts/folding.rs"]
mod folding;
#[path = "path_facts/guards.rs"]
mod guards;
#[path = "path_facts/joins.rs"]
mod joins;
#[path = "path_facts/masks.rs"]
mod masks;
#[path = "path_facts/representations.rs"]
mod representations;
#[path = "path_facts/sharing.rs"]
mod sharing;
#[path = "path_facts/snapshots.rs"]
mod snapshots;

use crate::{
    fixture::Fixture,
    wasm::{Input, Observation, TestModule, Value},
};
use wasm86_compiler::{Type, I1, I32, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn check_result(module: &TestModule, arguments: &[Value], expected: &[Value], v8: bool) {
    if v8 {
        assert_eq!(
            module.run_v8(&Input::call("run", arguments)),
            Observation::returned(expected)
        );
    } else {
        assert_eq!(
            module.instantiate().call_values("run", arguments),
            Ok(expected.to_vec())
        );
    }
}

fn count(module: &TestModule, predicate: impl Fn(&Operator<'_>) -> bool) -> usize {
    Validator::new().validate_all(module.bytes()).unwrap();
    Parser::new(0)
        .parse_all(module.bytes())
        .filter_map(|payload| {
            let Payload::CodeSectionEntry(body) = payload.unwrap() else {
                return None;
            };
            Some(
                body.get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .map(Result::unwrap)
                    .filter(&predicate)
                    .count(),
            )
        })
        .sum()
}

fn repeated_check() -> TestModule {
    Fixture::new().function(&[Type::I1], &[Type::I32], |mut body| {
        let flag = body.parameter::<I1>(0)?;
        body.if_(&flag, |arm| arm.return_(7))?;
        body.if_(&flag, |arm| arm.return_(11))?;
        body.return_(flag.select::<I32>(13, 17))
    })
}

fn rejected_disjunction() -> TestModule {
    Fixture::new().function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let first = body.parameter::<I1>(0)?;
        let second = body.parameter::<I1>(1)?;
        body.if_(first.or(&second), |arm| arm.return_(7))?;
        body.return_(first.select::<I32>(11, 13).add(second.select(17, 19)))
    })
}

fn masked_condition() -> TestModule {
    Fixture::new().function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let raised = body.parameter::<I1>(0)?;
        let unmasked = body.parameter::<I1>(1)?;
        body.if_(raised.and(unmasked), |arm| arm.return_(7))?;
        body.return_(raised.select::<I32>(11, 13))
    })
}

fn conditional_write() -> TestModule {
    Fixture::new().function(
        &[Type::I8, Type::I1, Type::I32],
        &[Type::I32, Type::I8],
        |mut body| {
            let previous = body.parameter::<I8>(0)?;
            let suppress = body.parameter::<I1>(1)?;
            let old = body.parameter::<I32>(2)?;
            let current = suppress.select(&old, old.add(1));
            let pending = suppress.select(1_u32, &previous);
            body.if_(pending.truncate::<I1>(), |arm| {
                arm.return_((&current, &pending))
            })?;
            body.return_((current, pending))
        },
    )
}

#[test]
fn a_returning_check_removes_repeated_checks_and_selections() {
    let module = repeated_check();
    assert_eq!(count(&module, |op| matches!(op, Operator::If { .. })), 1);
    assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 0);
    for (input, expected) in [(0, 17), (1, 7)] {
        assert_eq!(module.instantiate().call::<i32>((input,)), Ok(expected));
    }
}

#[test]
fn a_false_disjunction_proves_each_operand_false() {
    let module = rejected_disjunction();
    assert_eq!(count(&module, |op| matches!(op, Operator::Select)), 0);
    for a in 0..2 {
        for b in 0..2 {
            assert_eq!(
                module.instantiate().call::<i32>((a, b)),
                Ok(if a | b != 0 { 7 } else { 32 })
            );
        }
    }
}

#[test]
fn a_false_conjunction_does_not_prove_each_operand_false() {
    let module = masked_condition();
    for (a, b, expected) in [(0, 0, 13), (0, 1, 13), (1, 0, 11), (1, 1, 7)] {
        assert_eq!(module.instantiate().call::<i32>((a, b)), Ok(expected));
    }
}

#[test]
fn a_clear_low_bit_proves_a_conditional_write_without_erasing_upper_bits() {
    let module = conditional_write();
    for (previous, suppress, expected) in [
        (0, 0, (101, 0)),
        (0xfe, 0, (101, 0xfe)),
        (0x81, 0, (101, 0x81)),
        (0xfe, 1, (100, 1)),
    ] {
        assert_eq!(
            module
                .instantiate()
                .call::<(i32, i32)>((previous, suppress, 100)),
            Ok(expected)
        );
    }
}

#[test]
fn equality_facts_apply_only_in_the_proved_arm() {
    let module = Fixture::new().function(&[Type::I32], &[Type::I32], |mut body| {
        let value = body.parameter::<I32>(0)?;
        body.if_(value.eq(7), |arm| arm.return_(value.mul(3)))?;
        body.return_(value.add(1))
    });
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Mul)), 0);
    for (input, expected) in [(7, 21), (9, 10), (-1, 0)] {
        assert_eq!(module.instantiate().call::<i32>((input,)), Ok(expected));
    }
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_branch_facts_preserve_results_and_raw_bits() {
    let check = repeated_check();
    for (input, expected) in [(0, 17), (1, 7)] {
        assert_eq!(
            check.run_v8(&Input::call("run", &[Value::I32(input)])),
            Observation::returned(&[Value::I32(expected)])
        );
    }
    let write = conditional_write();
    for (previous, suppress, expected) in [
        (0xfe, 0, (101, 0xfe)),
        (0x81, 0, (101, 0x81)),
        (0xfe, 1, (100, 1)),
    ] {
        assert_eq!(
            write.run_v8(&Input::call(
                "run",
                &[Value::I32(previous), Value::I32(suppress), Value::I32(100)]
            )),
            Observation::returned(&[Value::I32(expected.0), Value::I32(expected.1)])
        );
    }
}
