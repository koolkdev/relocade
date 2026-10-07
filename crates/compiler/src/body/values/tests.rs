use super::*;
use crate::body::{Exit, FunctionGraph};

#[test]
fn interning_reuses_complete_results_after_storage_grows() {
    let mut values = ValueTable::default();
    let left = values.constant(Type::I64, 3);
    let right = values.constant(Type::I64, 5);
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
    assert_eq!(signed_root, 4);
    let initial_capacity = values.interned.capacity();
    let initial_value_capacity = values.values.capacity();
    // A separately placed calculation does not replace the canonical recipe.
    let placed = values.push(product);
    assert_ne!(root, placed);
    for bits in 0..1024 {
        values.constant(Type::I32, bits);
    }
    assert!(values.interned.capacity() > initial_capacity);
    assert!(values.values.capacity() > initial_value_capacity);

    let count = values.len();
    assert_eq!(values.intern(product), root);
    assert_eq!(values.intern(signed_product), signed_root);
    assert_eq!(values.expression_results(root), 2..4);
    assert_eq!(values.expression_result(root, 1), 3);
    assert!(matches!(
        values[3].definition,
        ValueDefinition::Result {
            producer: BlockItem::Evaluate(2),
            component: 1,
        }
    ));
    for bits in (0..1024).rev() {
        assert_eq!(values.constant(Type::I32, bits), 8 + bits as usize);
    }
    assert_eq!(values.len(), count);
}

#[test]
fn interning_distinguishes_logical_types_and_physical_bits() {
    let mut values = ValueTable::default();
    let byte = values.constant(Type::I8, 255);
    let word = values.constant(Type::I16, 255);
    let negative_byte = values.carrier_constant(Type::I8, 0xffff_ffff);

    assert_eq!([byte, word, negative_byte], [0, 1, 2]);
    assert_eq!(values.constant(Type::I8, 255), byte);
    assert_eq!(values.constant(Type::I16, 255), word);
    assert_eq!(
        values.carrier_constant(Type::I8, 0xffff_ffff),
        negative_byte
    );
    assert_eq!(values.len(), 3);
}

#[test]
fn finalization_releases_the_interning_allocation() {
    let mut graph = FunctionGraph::new();
    graph.values.constant(Type::I32, 13);
    let result = graph.values.constant(Type::I32, 7);
    graph.blocks[0].exit = Exit::Return(vec![result]);
    assert!(graph.values.interned.capacity() > 0);

    graph.compact(|_| false);

    assert_eq!(graph.values.interned.capacity(), 0);
    assert_eq!(graph.values.len(), 1);
    assert!(matches!(
        graph.values[0].definition,
        ValueDefinition::Constant(7)
    ));
}
