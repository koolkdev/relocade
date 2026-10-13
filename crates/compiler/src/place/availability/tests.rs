use super::*;
use crate::expression::{BinaryOp as Binary, UnaryOp as Unary};

#[test]
fn execution_reuse_and_bindings_end_together_at_scope_exit() {
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
    let count = graph.values.len();
    available.evaluate(&mut graph, BlockId(0), product);
    let placed_low = available.get(product).unwrap();
    let placed_high = available.get(high).unwrap();
    assert_eq!(graph.values.len(), count);
    assert_eq!(placed_low, product);
    assert_eq!(placed_high, high);
    assert_eq!(available.lookup(&graph.values, high), Some(placed_high));
    assert_ne!(placed_low, placed_high);
    available.evaluate(&mut graph, BlockId(0), product);
    assert_eq!(graph.values.len(), count);
    assert_eq!(graph.blocks[0].items.len(), 1);
    available.restore(parent);
    assert_eq!(available.lookup(&graph.values, product), None);
    assert_eq!(available.lookup(&graph.values, high), None);

    let sibling = graph.block(0, &[]);
    available.evaluate(&mut graph, sibling, product);
    let sibling_low = available.get(product).unwrap();
    let sibling_high = available.get(high).unwrap();
    assert_eq!(graph.values.len(), count + 2);
    assert_ne!(sibling_low, placed_low);
    assert_ne!(sibling_high, placed_high);
    assert_eq!(graph.values.expression_result(sibling_low, 1), sibling_high);
    assert_eq!(graph.blocks[sibling.0].items.len(), 1);
}

#[test]
fn rebinding_inputs_preserves_the_recipe_for_a_later_unchanged_placement() {
    let mut graph = FunctionGraph::new();
    for component in 0..2 {
        let parameter = graph.values.push(Value {
            ty: crate::Type::I64,
            definition: ValueDefinition::Parameter {
                block: graph.entry,
                component,
            },
        });
        graph.blocks[0].parameters.push(parameter);
    }
    let [input, replacement] = graph.blocks[0].parameters[..] else {
        unreachable!()
    };
    let recipe = Value {
        ty: crate::Type::I64,
        definition: ValueDefinition::Expression(Expression::Binary {
            operator: Binary::Integer(crate::integer::BinaryOp::Mul),
            left: input,
            right: input,
        }),
    };
    let product = graph.values.intern(recipe);
    let mut available = Availability::default();
    let parent = available.checkpoint();
    let count = graph.values.len();
    available.bind(input, replacement);
    available.evaluate(&mut graph, BlockId(0), product);
    let placed = available.get(product).unwrap();
    assert_ne!(placed, product);
    assert_eq!(graph.values.len(), count + 1);
    assert!(graph.values[product] == recipe);
    assert_eq!(
        graph
            .inputs(BlockItem::Evaluate(placed))
            .collect::<Vec<_>>(),
        [replacement, replacement]
    );

    available.restore(parent);
    let sibling = graph.block(0, &[]);
    available.evaluate(&mut graph, sibling, product);
    assert_eq!(available.get(product), Some(product));
    assert_eq!(graph.values.len(), count + 1);
    assert!(graph.values[product] == recipe);
}

#[test]
fn fresh_executions_keep_the_lowered_recipes_result_bounds() {
    use crate::{integer::BitCountOp, Type};
    for remap_input in [false, true] {
        let mut graph = FunctionGraph::new();
        let input = graph.values.push_with_bounds(
            Value {
                ty: Type::I8,
                definition: ValueDefinition::Parameter {
                    block: graph.entry,
                    component: 0,
                },
            },
            BitBounds {
                unsigned: 8,
                signed: 9,
            },
        );
        let count = crate::expression::build(
            &mut graph.values,
            Type::I8,
            Expression::Unary {
                operator: Unary::BitCount {
                    operator: BitCountOp::LeadingZeros,
                    input_type: Type::I8,
                },
                input,
            },
            0,
        );
        let lowered = crate::expression::lower(&mut graph.values, count);
        let adjustment = graph.values.representation(lowered);
        let mut available = Availability::default();
        let parent = available.checkpoint();
        available.evaluate(&mut graph, BlockId(0), adjustment);
        available.restore(parent);
        if remap_input {
            let count = *graph
                .values
                .expression(adjustment)
                .unwrap()
                .expression
                .inputs()
                .next()
                .unwrap();
            let replacement = graph.values.push(graph.values[count]);
            available.bind(count, replacement);
        }
        let sibling = graph.block(0, &[]);
        available.evaluate(&mut graph, sibling, adjustment);
        let placed = available.get(adjustment).unwrap();
        assert_ne!(placed, adjustment);
        assert_eq!(graph.values.bounds[placed].unsigned, 4);
        assert_eq!(graph.values.bounds[placed].signed, 5);
    }
}

#[test]
fn logical_input_widths_survive_same_carrier_aliases() {
    use crate::{integer::BitCountOp, Type};
    fn check(
        bits: u64,
        ty: Type,
        expression: impl FnOnce(usize, &mut ValueTable) -> Expression<usize>,
        expected: u128,
    ) {
        let mut graph = FunctionGraph::new();
        let source = graph.values.literal(Type::I32, bits);
        let byte = graph.values.intern(Value {
            ty: Type::I8,
            definition: ValueDefinition::Expression(Expression::Unary {
                operator: Unary::Convert,
                input: source,
            }),
        });
        let expression = expression(byte, &mut graph.values);
        let consumer = graph.values.intern(Value {
            ty,
            definition: ValueDefinition::Expression(expression),
        });
        // Joins may reuse an identical carrier with a different logical type.
        let mut available = Availability::default();
        available.bind(byte, source);
        available.evaluate(&mut graph, BlockId(0), consumer);
        let placed = available.get(consumer).unwrap();
        let result = crate::expression::refold(&mut graph.values, placed);
        let ValueDefinition::Literal(bits) = graph.values[result].definition else {
            panic!("the resolved literal input folds");
        };
        assert_eq!(u128::from(bits), expected);
    }
    check(
        0x34,
        Type::I8,
        |input, _| Expression::Unary {
            operator: Unary::BitCount {
                operator: BitCountOp::LeadingZeros,
                input_type: Type::I8,
            },
            input,
        },
        2,
    );
    check(
        0x80,
        Type::I32,
        |input, _| Expression::Unary {
            operator: Unary::SignExtend {
                input_type: Type::I8,
            },
            input,
        },
        0xffff_ff80,
    );
    check(
        0x34,
        Type::V128,
        |input, values| Expression::Binary {
            operator: Binary::ReplaceLane {
                lane_type: Type::I8,
                lane: 3,
            },
            left: values.literal(Type::V128, 0_u128),
            right: input,
        },
        0x34_u128 << 24,
    );
}
