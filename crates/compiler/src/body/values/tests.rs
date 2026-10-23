use super::*;
use crate::body::{Exit, FunctionGraph};

#[test]
fn interning_reuses_complete_results_after_storage_grows() {
    let mut values = ValueTable::default();
    let left = values.literal(Type::I64, 3);
    let right = values.literal(Type::I64, 5);
    let product = Value {
        ty: Type::I64,
        definition: ValueDefinition::Expression(Expression::MultiplyWide {
            signed: false,
            left,
            right,
        }),
    };
    let root = values.intern(product);
    let signed_product = Value {
        ty: Type::I64,
        definition: ValueDefinition::Expression(Expression::MultiplyWide {
            signed: true,
            left,
            right,
        }),
    };
    let signed_root = values.intern(signed_product);
    assert_ne!(signed_root, root);
    let initial_capacity = values.interned.capacity();
    let initial_value_capacity = values.values.capacity();
    // A separately placed calculation does not replace the canonical recipe.
    let placed = values.push(product);
    assert_ne!(root, placed);
    let literals: Vec<_> = (0..1024)
        .map(|bits| (bits, values.literal(Type::I32, bits)))
        .collect();
    assert!(values.interned.capacity() > initial_capacity);
    assert!(values.values.capacity() > initial_value_capacity);

    let count = values.len();
    assert_eq!(values.intern(product), root);
    assert_eq!(values.intern(signed_product), signed_root);
    assert_eq!(values.expression_results(root), root..root + 2);
    assert_eq!(values.expression_result(root, 1), root + 1);
    assert!(matches!(
        values[root + 1].definition,
        ValueDefinition::Result {
            producer: BlockItem::Evaluate(producer),
            component: 1,
        } if producer == root
    ));
    for (bits, id) in literals.into_iter().rev() {
        assert_eq!(values.literal(Type::I32, bits), id);
        assert_eq!(values[id].scalar_literal(), Some(bits));
    }
    assert_eq!(values.len(), count);
}

#[test]
fn interning_distinguishes_logical_types_and_physical_bits() {
    let mut values = ValueTable::default();
    let byte = values.literal(Type::I8, 255);
    let word = values.literal(Type::I16, 255);
    let negative_byte = values.carrier_literal(Type::I8, 0xffff_ffff_u64);

    assert_ne!(byte, word);
    assert_ne!(byte, negative_byte);
    assert_ne!(word, negative_byte);
    assert_eq!(values.literal(Type::I8, 255), byte);
    assert_eq!(values.literal(Type::I16, 255), word);
    assert_eq!(
        values.carrier_literal(Type::I8, 0xffff_ffff_u64),
        negative_byte
    );
    assert_eq!(values.len(), 3);
}

#[test]
fn finalization_releases_the_interning_allocation() {
    let mut graph = FunctionGraph::new();
    graph.values.literal(Type::I32, 13);
    let result = graph.values.literal(Type::I32, 7);
    graph.blocks[0].exit = Exit::Return(vec![result]);
    assert!(graph.values.interned.capacity() > 0);

    graph.compact(|_| false);

    assert_eq!(graph.values.interned.capacity(), 0);
    assert_eq!(graph.values.len(), 1);
    assert!(matches!(graph.values[0].scalar_literal(), Some(7)));
}
