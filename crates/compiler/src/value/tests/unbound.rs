use std::{cell::Cell, rc::Rc};

use super::Val;
use crate::{arena::ExpressionArena, BuildError, Type, I32};

#[test]
fn a_shared_unbound_dag_is_resolved_once_per_arena_and_released_when_closed() {
    let builds = Rc::new(Cell::new(0));
    let count = builds.clone();
    let base = Val::<I32>::from(7).unsigned().div(0);
    let mut expression = Val::<I32>::unbound(move |arena| {
        count.set(count.get() + 1);
        // Bound a broken traversal before it expands the shared DAG exponentially.
        assert!(count.get() <= 2, "repeatedly resolved a shared recipe");
        base.checked_expression(arena, 0)
    });
    for _ in 0..40 {
        expression = expression.add(&expression);
    }
    let folded = expression.and(0);
    let first = ExpressionArena::new();
    let second = ExpressionArena::new();
    for (arena, expected_builds) in [(&first, 1), (&second, 2)] {
        for _ in 0..2 {
            let value = folded.checked_expression(arena, 0).unwrap();
            assert_eq!(value, arena.constant(Type::I32, 0).unwrap());
            assert_eq!(builds.get(), expected_builds);
        }
    }
    assert!(first.take().is_some());
    assert_eq!(
        folded.checked_expression(&first, 0),
        Err(BuildError::BodyClosed)
    );
    assert_eq!(builds.get(), 2);
    drop(folded);
    drop(expression);
    // Only the other open arena still retains the recipe graph.
    assert_eq!(Rc::strong_count(&builds), 2);
    assert!(second.take().is_some());
    assert_eq!(Rc::strong_count(&builds), 1);
}
