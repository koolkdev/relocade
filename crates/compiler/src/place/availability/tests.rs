use super::*;

#[test]
fn aliases_wait_for_execution_and_survive_in_each_dominated_branch() {
    let mut available = Availability::default();
    available.record_aliases([Alias {
        recipe: 1,
        residual: 2,
    }]);
    assert_eq!(available.get(1), None);
    let parent = available.checkpoint();
    available.bind(2, 3);
    assert_eq!(available.get(1), Some(3));
    available.restore(parent);
    assert_eq!(available.get(1), None);
    available.bind(2, 4);
    assert_eq!(available.get(1), Some(4));
}

#[test]
fn aliases_of_executed_values_are_available_immediately() {
    let mut available = Availability::default();
    available.bind(2, 3);
    assert_eq!(available.get(3), Some(3));
    available.record_aliases([Alias {
        recipe: 1,
        residual: 2,
    }]);
    assert_eq!(available.get(1), Some(3));
}

#[test]
fn leaving_a_branch_removes_its_aliases_and_restores_existing_bindings() {
    let mut available = Availability::default();
    available.bind(0, 1);
    let parent = available.checkpoint();
    available.record_aliases([Alias {
        recipe: 2,
        residual: 3,
    }]);
    available.bind(0, 4);
    available.bind(3, 5);
    assert_eq!(available.get(2), Some(5));
    available.restore(parent);
    assert_eq!(available.get(0), Some(1));
    available.bind(3, 6);
    assert_eq!(available.get(2), None);
}

#[test]
fn chains_of_aliases_publish_without_replacing_known_values() {
    let mut available = Availability::default();
    available.bind(0, 7);
    available.record_aliases([
        Alias {
            recipe: 0,
            residual: 1,
        },
        Alias {
            recipe: 1,
            residual: 2,
        },
        Alias {
            recipe: 2,
            residual: 3,
        },
    ]);
    available.bind(3, 4);
    assert_eq!(available.get(0), Some(7));
    assert_eq!(available.get(1), Some(4));
    assert_eq!(available.get(2), Some(4));
}

#[test]
fn execution_reuse_and_aliases_end_together_at_scope_exit() {
    let mut graph = FunctionGraph::new();
    let input = graph.values.push(Value {
        ty: crate::Type::I64,
        definition: ValueDefinition::Parameter {
            block: graph.entry,
            component: 0,
        },
    });
    let product = graph.values.intern(Value {
        ty: crate::Type::I64,
        definition: ValueDefinition::Expression(Expression::MultiplyWide {
            signed: false,
            left: input,
            right: input,
        }),
    });
    let high = graph.values.expression_result(product, 1);
    let mut available = Availability::default();
    let parent = available.checkpoint();
    available.evaluate(&mut graph, BlockId(0), product);
    let placed_low = available.get(product).unwrap();
    let placed_high = available.get(high).unwrap();
    assert_eq!(available.lookup(&graph.values, high), Some(placed_high));
    assert_ne!(placed_low, placed_high);
    assert_eq!(graph.blocks[0].items.len(), 1);
    available.restore(parent);
    assert_eq!(available.lookup(&graph.values, product), None);
    assert_eq!(available.lookup(&graph.values, high), None);
}

#[test]
fn publishing_an_alias_chain_keeps_intermediate_width_limits_for_facts() {
    enum BindingOrder {
        ResidualBeforeAlias,
        ResidualAfterAlias,
        OriginalAfterAliases,
    }
    let mut values = ValueTable::default();
    let inputs: Vec<_> = [crate::Type::I32, crate::Type::I8, crate::Type::I32]
        .into_iter()
        .enumerate()
        .map(|(component, ty)| {
            values.push(Value {
                ty,
                definition: ValueDefinition::Parameter {
                    block: BlockId(0),
                    component,
                },
            })
        })
        .collect();
    let [original, narrow, executed] = inputs[..] else {
        unreachable!()
    };
    for order in [
        BindingOrder::ResidualBeforeAlias,
        BindingOrder::ResidualAfterAlias,
        BindingOrder::OriginalAfterAliases,
    ] {
        let mut available = Availability::default();
        if matches!(order, BindingOrder::ResidualBeforeAlias) {
            available.bind(narrow, executed);
        }
        available.record_aliases([Alias {
            recipe: original,
            residual: narrow,
        }]);
        match order {
            BindingOrder::ResidualBeforeAlias => {}
            BindingOrder::ResidualAfterAlias => available.bind(narrow, executed),
            BindingOrder::OriginalAfterAliases => {
                available.record_aliases([Alias {
                    recipe: narrow,
                    residual: executed,
                }]);
                available.bind(original, executed);
            }
        }
        assert_eq!(available.get(original), Some(executed));
        let known = available
            .sources(&values, executed, u64::from(u32::MAX))
            .into_iter()
            .filter(|source| source.recipe == original)
            .fold(0, |mask, source| mask | source.mask);
        assert_eq!(known, 0xff);
    }
}
