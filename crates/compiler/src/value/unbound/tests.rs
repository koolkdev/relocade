use super::*;
use crate::{body::ValueDefinition, value::source::ValueSource, IntType, Val, I1, I32, I64, I8};

#[test]
fn shared_unbound_expressions_are_cached_per_body_and_released_when_closed() {
    let mut expression = Val::<I32>::from(7).unsigned().div(0);
    for _ in 0..40 {
        expression = expression.add(&expression);
    }
    let folded = expression.and(0);
    let ValueSource::Unbound(recipe) = &folded.source else {
        panic!("the expression remains unbound before admission");
    };
    let retained = Rc::downgrade(&recipe.0);
    assert_eq!(retained.strong_count(), 1);
    let first = ExpressionArena::new();
    let second = ExpressionArena::new();
    for (arena, owners) in [(&first, 2), (&second, 3)] {
        for _ in 0..2 {
            let value = folded.checked_expression(arena, 0).unwrap();
            assert_eq!(value, arena.constant(Type::I32, 0).unwrap());
            assert_eq!(retained.strong_count(), owners);
        }
    }
    let _first_values = first.take().unwrap();
    assert_eq!(
        folded.checked_expression(&first, 0),
        Err(BuildError::BodyClosed)
    );
    assert_eq!(retained.strong_count(), 2);
    drop(folded);
    drop(expression);
    // Only the second body's cache retains the expression graph.
    assert_eq!(retained.strong_count(), 1);
    let _second_values = second.take().unwrap();
    assert!(retained.upgrade().is_none());
}

#[test]
fn unbound_identity_tracks_shared_nodes_and_admission_uses_body_canonicalization() {
    let first = Val::<I32>::from(7).unsigned().div(0);
    let copy = first.clone();
    let separate = Val::<I32>::from(7).unsigned().div(0);
    assert!(first.same_expression(&copy));
    assert!(!first.same_expression(&separate));
    let arena = ExpressionArena::new();
    let first = first.bind(&arena, 0).unwrap();
    let separate = separate.bind(&arena, 0).unwrap();
    assert!(first.same_expression(&separate));
}

#[test]
fn unbound_operands_keep_their_types_across_comparisons_shifts_and_conversions() {
    // Division by zero cannot be folded without a body. Admission of the mask
    // removes it, making the remaining expected results ordinary constants.
    let wide_zero = Val::<I64>::from(7_u64).unsigned().div(0).and(0);
    let count = Val::<I32>::from(7).unsigned().div(0).and(0).add(40);
    let narrow = Val::<I8>::from(7).unsigned().div(0).and(0).or(0x80);
    let negative = wide_zero.or(1_u64 << 63).signed().lt(0);
    let shifted = wide_zero.or(1).shl(count);
    let extended = narrow.signed().extend::<I64>();
    let chosen = negative.select(shifted.clone(), extended.clone());
    let arena = ExpressionArena::new();
    let expected = [
        (negative.checked_expression(&arena, 0).unwrap(), I1::TYPE, 1),
        (
            shifted.checked_expression(&arena, 0).unwrap(),
            I64::TYPE,
            1 << 40,
        ),
        (
            extended.checked_expression(&arena, 0).unwrap(),
            I64::TYPE,
            (-128_i64) as u64,
        ),
        (
            chosen.checked_expression(&arena, 0).unwrap(),
            I64::TYPE,
            1 << 40,
        ),
    ];
    let values = arena.take().unwrap().values;
    for (id, ty, bits) in expected {
        assert_eq!(values[id].ty, ty);
        assert!(
            matches!(values[id].definition, ValueDefinition::Constant(actual) if actual == bits)
        );
    }
}
