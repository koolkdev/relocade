use super::*;

#[test]
fn leaving_a_branch_restores_live_facts_and_preserves_its_snapshot() {
    let mut graph = SsaGraph::new();
    let value = graph.values.push(Value {
        ty: Type::I32,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component: 0,
        },
    });
    let incoming = vec![vec![], vec![0], vec![0], vec![1, 2]];
    let dominators = Dominators::new(0, &[vec![1, 2], vec![3], vec![3], vec![]], &incoming);
    let mut scopes = Scopes::new(&dominators, &incoming, graph.values.len());
    let predicate = scopes.predicate_id(Predicate::Zero(value));
    let entry = scopes.enter(BlockId(0));
    let parent = scopes.refine(&mut graph, value);
    scopes.finish();

    let left = scopes.enter(BlockId(1));
    let left_value = scopes.refine(&mut graph, parent);
    scopes.assume_predicate(&graph, predicate, false);
    assert_eq!(scopes.refinement(value), Some(left_value));
    assert_eq!(graph.values.definition_block(left_value), Some(BlockId(1)));
    scopes.finish();
    scopes.leave(left);

    assert_eq!(scopes.refinement(value), Some(parent));
    assert_eq!(scopes.predicate(predicate), None);
    let right = scopes.enter(BlockId(2));
    let right_value = scopes.refine(&mut graph, parent);
    scopes.assume_predicate(&graph, predicate, true);
    scopes.finish();
    scopes.leave(right);

    assert_eq!(scopes.value_at(1, value, parent), left_value);
    assert_eq!(scopes.value_at(2, value, parent), right_value);
    assert_eq!(scopes.predicate_at(1, predicate), Some(false));
    assert_eq!(scopes.predicate_at(2, predicate), Some(true));
    assert_eq!(scopes.refinement(value), Some(parent));
    assert_eq!(scopes.predicate(predicate), None);

    let join = scopes.enter(BlockId(3));
    let pending = scopes.pending();
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&FactKey::Value(value)));
    assert!(pending.contains(&FactKey::Predicate(predicate)));
    assert!(scopes.pending().is_empty());
    scopes.leave(join);
    scopes.leave(entry);
    assert_eq!(scopes.refinement(value), None);
}
