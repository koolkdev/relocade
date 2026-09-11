use super::{operators, Kind, KINDS};
use crate::fixture::{signature, Fixture};
use wasm86_compiler::{BuildError, Program, Type, Val, I1, I32, I64};
use wasmparser::Operator;

#[test]
fn division_and_remainder_preserve_foreign_operand_validation() {
    for kind in KINDS {
        let mut program = Program::new();
        let function = program.declare(signature(&[], &[Type::I32]));
        let body = program.define(function).unwrap();
        let foreign = body.value::<I32>(1).unwrap();
        drop(body);
        let body = program.define(function).unwrap();
        let cached = Val::<I32>::from(7).unsigned().div(0).and(0);
        body.value(&cached).unwrap();
        assert_eq!(
            body.value(cached.add(&foreign).mul(0)).err(),
            Some(BuildError::ForeignBody)
        );
        for literal in [0, 1] {
            assert_eq!(
                body.value(kind.apply(&foreign, literal)).err(),
                Some(BuildError::ForeignBody)
            );
            assert_eq!(
                body.value(kind.apply(literal, &foreign)).err(),
                Some(BuildError::ForeignBody)
            );
        }
        let result = kind.apply(body.value::<I32>(0).unwrap(), 1);
        body.return_(0).unwrap();
        let next_function = program.declare(signature(&[], &[Type::I32]));
        let next = program.define(next_function).unwrap();
        assert_eq!(
            next.value(result.add(1)).err(),
            Some(BuildError::ForeignBody)
        );
        next.return_(0).unwrap();
        assert!(program.compile().is_ok());
    }
}

#[test]
fn folded_division_operands_and_results_keep_the_original_child_visibility() {
    for kind in KINDS {
        let mut fixture = Fixture::new();
        let memory = fixture.memory("state", &[7, 0, 0, 0]);
        let module = fixture.function(&[], &[Type::I32], |mut body| {
            let mut retained = None;
            body.if_(false, |mut child| {
                let input = child.load::<I32>(memory, 0)?.and(0).add(1);
                let result = kind.apply(&input, 1);
                let expected = if matches!(kind, Kind::DivUnsigned | Kind::DivSigned) {
                    1
                } else {
                    0
                };
                assert!(result.same_expression(&child.value::<I32>(expected)?));
                retained = Some((input, result));
                Ok(())
            })?;
            let (child_input, child_result) = retained.unwrap();
            let cached = Val::<I32>::from(7).unsigned().div(0).and(0);
            body.value(&cached)?;
            assert_eq!(
                body.value(cached.add(&child_input).mul(0)).err(),
                Some(BuildError::OutOfScope)
            );
            assert_eq!(
                body.value(&child_result).err(),
                Some(BuildError::OutOfScope)
            );
            assert_eq!(
                body.value(kind.apply(&child_input, 1)).err(),
                Some(BuildError::OutOfScope)
            );
            assert_eq!(
                body.value(kind.apply(0, &child_input)).err(),
                Some(BuildError::OutOfScope)
            );
            body.if_(false, |sibling| {
                assert_eq!(
                    sibling.value(kind.apply(&child_result, 1)).err(),
                    Some(BuildError::OutOfScope)
                );
                assert_eq!(
                    sibling.value(kind.apply(0, &child_result)).err(),
                    Some(BuildError::OutOfScope)
                );
                Ok(())
            })?;
            body.return_(23)
        });
        assert_eq!(module.instantiate().call::<i32>(()).unwrap(), 23);
    }
}

#[test]
fn nonfolding_standalone_constants_form_reusable_native_expressions() {
    let expressions = [
        Val::<I32>::from(7).unsigned().div(0),
        Val::<I32>::from(7).signed().div(0),
        Val::<I32>::from(7).unsigned().rem(0),
        Val::<I32>::from(7).signed().rem(0),
        Val::<I32>::from(i32::MIN).signed().div(-1),
    ];
    for expression in expressions {
        assert!(expression.same_expression(&expression.clone()));
        for _ in 0..2 {
            let module = Fixture::new().function(&[], &[Type::I32], |body| {
                let first = body.value(&expression)?;
                let second = body.value(&expression)?;
                assert!(first.same_expression(&second));
                body.return_(first)
            });
            assert!(operators(module.bytes()).iter().any(|op| matches!(
                op,
                Operator::I32DivU | Operator::I32DivS | Operator::I32RemU | Operator::I32RemS
            )));
        }
    }
    let expression = Val::<I64>::from(0x8000_0000_0000_0000_u64).signed().div(-1);
    let module = Fixture::new().expression(&[], |_| expression);
    assert!(operators(module.bytes())
        .iter()
        .any(|op| matches!(op, Operator::I64DivS)));
}

#[test]
fn unbound_calculations_compose_through_unary_binary_and_value_selection_owners() {
    let expression = Val::<I32>::from(7).unsigned().div(0);
    let mapped = expression
        .signed()
        .extend::<I64>()
        .truncate::<I32>()
        .popcnt();
    let combined = mapped.add(&expression).shl(1).rotl(3);
    let condition = combined.eq(1);
    let result = condition.select(expression.sub(1), combined);
    let mut bytes = None;
    for _ in 0..2 {
        let module = Fixture::new().function(&[], &[Type::I32], |body| {
            let first = body.value(&result)?;
            assert!(first.same_expression(&body.value(&result)?));
            body.return_(first)
        });
        if let Some(previous) = &bytes {
            assert_eq!(module.bytes(), previous);
        } else {
            bytes = Some(module.bytes().to_vec());
        }
        let code = operators(module.bytes());
        assert!(code.iter().any(|op| matches!(op, Operator::I32DivU)));
        assert!(code.iter().any(|op| matches!(op, Operator::I32Popcnt)));
        assert!(code.iter().any(|op| matches!(op, Operator::Select)));
    }
    // Admitting an unbound calculation still checks every already-bound operand.
    let module = Fixture::new().function(&[Type::I32], &[Type::I32], |body| {
        let parameter = body.parameter::<I32>(0)?;
        let result = expression.add(&parameter).mul(0);
        let chosen = Val::<I1>::from(false).select(&expression, parameter);
        body.return_(result.add(chosen))
    });
    assert_eq!(module.instantiate().call::<i32>(29).unwrap(), 29);
}
