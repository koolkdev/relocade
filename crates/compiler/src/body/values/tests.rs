use super::*;
use crate::body::{Exit, FunctionGraph};

#[test]
fn finalization_releases_the_interning_allocation() {
    let mut graph = FunctionGraph::new();
    graph.values.constant(Type::I32, 13);
    let result = graph.values.constant(Type::I32, 7);
    graph.blocks[0].exit = Exit::Return(vec![result]);
    assert!(graph.values.interned.capacity() > 0);

    graph.compact(vec![false, true]);

    assert_eq!(graph.values.interned.capacity(), 0);
    assert_eq!(graph.values.len(), 1);
    assert!(matches!(
        graph.values[0].definition,
        ValueDefinition::Constant(7)
    ));
}
