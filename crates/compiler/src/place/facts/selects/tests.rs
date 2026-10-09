use super::*;
use crate::{
    bitwise::BitwiseOp,
    body::{BlockId, Value},
    integer::{BinaryOp, CompareOp},
};

fn expression(table: &mut ValueTable, ty: Type, expression: Expression<usize>) -> usize {
    table.intern(Value {
        ty,
        definition: ValueDefinition::Expression(expression),
    })
}

fn parameter(table: &mut ValueTable, ty: Type, component: usize) -> usize {
    table.push(Value {
        ty,
        definition: ValueDefinition::Parameter {
            block: BlockId(0),
            component,
        },
    })
}

fn compare(table: &mut ValueTable, operator: CompareOp, left: usize, right: usize) -> usize {
    expression(
        table,
        Type::I1,
        Expression::Compare {
            operator,
            left,
            right,
        },
    )
}

fn correlated(table: &mut ValueTable, first: usize, second: usize, input: usize) -> usize {
    let zero = table.literal(Type::I64, 0);
    let top = table.literal(Type::I64, 1_u64 << 63);
    let set = expression(
        table,
        Type::I64,
        Expression::Bitwise {
            operator: BitwiseOp::Or,
            left: input,
            right: top,
        },
    );
    let a = expression(
        table,
        Type::I64,
        Expression::Select {
            condition: first,
            when_true: zero,
            when_false: set,
        },
    );
    let b = expression(
        table,
        Type::I64,
        Expression::Select {
            condition: second,
            when_true: zero,
            when_false: input,
        },
    );
    let top_bit = expression(
        table,
        Type::I64,
        Expression::Bitwise {
            operator: BitwiseOp::And,
            left: a,
            right: top,
        },
    );
    let clear = expression(
        table,
        Type::I1,
        Expression::ZeroTest {
            input: top_bit,
            nonzero: false,
        },
    );
    let nonzero = expression(
        table,
        Type::I1,
        Expression::ZeroTest {
            input: b,
            nonzero: true,
        },
    );
    expression(
        table,
        Type::I1,
        Expression::Bitwise {
            operator: BitwiseOp::And,
            left: clear,
            right: nonzero,
        },
    )
}

#[test]
fn speculative_select_facts_do_not_escape_or_survive_a_scope_change() {
    let mut table = ValueTable::default();
    let first = parameter(&mut table, Type::I1, 0);
    let second = parameter(&mut table, Type::I1, 1);
    let input = parameter(&mut table, Type::I64, 2);
    let invariant = correlated(&mut table, first, first, input);
    let independent = correlated(&mut table, first, second, input);
    let mut facts = ScalarFacts::default();
    assert_eq!(facts.constant_across_selects(&table, invariant), Some(0));
    assert_eq!(facts.constant(&table, first), None);
    assert_eq!(facts.constant_across_selects(&table, independent), None);
    let scope = facts.checkpoint();
    facts.assume(&table, first, true);
    facts.assume(&table, second, false);
    facts.assume_bits(input, 1, 1);
    assert_eq!(facts.constant_across_selects(&table, independent), Some(1));
    let mut snapshot = facts.clone();
    facts.restore(scope);
    assert_eq!(facts.constant_across_selects(&table, independent), None);
    facts.assume(&table, second, true);
    assert_eq!(facts.constant_across_selects(&table, independent), Some(0));
    assert_eq!(
        snapshot.constant_across_selects(&table, independent),
        Some(1)
    );
}

#[test]
fn successful_and_failed_proofs_preserve_inherited_facts_and_cached_bits() {
    let mut table = ValueTable::default();
    let selector_input = parameter(&mut table, Type::I32, 0);
    let [ten, twenty, thirty, forty] =
        [10, 20, 30, 40].map(|value| table.literal(Type::I32, value));
    let condition = compare(&mut table, CompareOp::LtUnsigned, selector_input, twenty);
    let inherited = compare(&mut table, CompareOp::LtUnsigned, selector_input, forty);
    let other = parameter(&mut table, Type::I1, 1);
    let input = parameter(&mut table, Type::I64, 2);
    let invariant = correlated(&mut table, condition, condition, input);
    let dynamic = expression(
        &mut table,
        Type::I1,
        Expression::Bitwise {
            operator: BitwiseOp::Or,
            left: invariant,
            right: other,
        },
    );
    let mut facts = ScalarFacts::default();
    let scope = facts.checkpoint();
    facts.assume(&table, inherited, true);
    facts.assume_bits(input, 0xff, 0x42);
    facts.bits(&table, dynamic);
    let cached = facts.computed.borrow().clone();
    assert_eq!(facts.constant_across_selects(&table, invariant), Some(0));
    assert_eq!(facts.constant_across_selects(&table, dynamic), None);
    assert!(*facts.computed.borrow() == cached);
    assert_eq!(facts.bits(&table, input).value, 0x42);
    assert_eq!(facts.constant(&table, condition), None);
    assert_eq!(facts.constant(&table, other), None);
    // Fresh predicates must not see comparison or range facts from either case.
    let opposite = compare(&mut table, CompareOp::GeUnsigned, selector_input, twenty);
    let below_ten = compare(&mut table, CompareOp::LtUnsigned, selector_input, ten);
    let below_thirty = compare(&mut table, CompareOp::LtUnsigned, selector_input, thirty);
    assert_eq!(facts.constant(&table, opposite), None);
    assert_eq!(facts.constant(&table, below_ten), None);
    assert_eq!(facts.constant(&table, below_thirty), None);
    assert_eq!(facts.constant(&table, inherited), Some(1));
    facts.restore(scope);
    assert_eq!(facts.bits(&table, input).mask, 0);
    let outside = compare(&mut table, CompareOp::GeUnsigned, selector_input, forty);
    assert_eq!(facts.constant(&table, outside), None);
}

#[test]
fn unknown_selects_keep_only_bits_shared_by_both_alternatives() {
    let mut table = ValueTable::default();
    let condition = parameter(&mut table, Type::I1, 0);
    let a = table.literal(Type::I64, 0x93);
    let b = table.literal(Type::I64, 0x95);
    let choice = expression(
        &mut table,
        Type::I64,
        Expression::Select {
            condition,
            when_true: a,
            when_false: b,
        },
    );
    let facts = ScalarFacts::default();
    let bits = facts.bits(&table, choice);
    assert_eq!((bits.mask, bits.value), (!6_u64, 0x91));
    assert_eq!(facts.constant(&table, choice), None);
}

#[test]
fn equal_logical_select_bits_do_not_make_the_carrier_constant() {
    let mut table = ValueTable::default();
    let condition = parameter(&mut table, Type::I1, 0);
    let a = table.carrier_literal(Type::I8, 0x100);
    let b = table.carrier_literal(Type::I8, 0x200);
    let choice = expression(
        &mut table,
        Type::I8,
        Expression::Select {
            condition,
            when_true: a,
            when_false: b,
        },
    );
    let carrier = expression(&mut table, Type::I32, Expression::Convert { input: choice });
    let facts = ScalarFacts::default();
    assert_eq!(facts.inferred_constant(&table, choice), Some(0));
    assert_eq!(table.carrier_bits(choice, 0), None);
    assert_eq!(facts.inferred_constant(&table, carrier), None);
}

#[test]
fn select_proofs_use_ranges_without_leaking_case_assumptions() {
    let mut table = ValueTable::default();
    let input = parameter(&mut table, Type::I32, 0);
    let twenty = table.literal(Type::I32, 20);
    let twenty_one = table.literal(Type::I32, 21);
    let one = table.literal(Type::I1, 1);
    let condition = compare(&mut table, CompareOp::LtUnsigned, input, twenty_one);
    let is_twenty = compare(&mut table, CompareOp::Eq, input, twenty);
    let at_least_twenty_one = compare(&mut table, CompareOp::GeUnsigned, input, twenty_one);
    let at_least_twenty = compare(&mut table, CompareOp::GeUnsigned, input, twenty);
    let a = expression(
        &mut table,
        Type::I1,
        Expression::Select {
            condition,
            when_true: is_twenty,
            when_false: one,
        },
    );
    let b = expression(
        &mut table,
        Type::I1,
        Expression::Select {
            condition,
            when_true: one,
            when_false: at_least_twenty_one,
        },
    );
    let both = expression(
        &mut table,
        Type::I1,
        Expression::Bitwise {
            operator: BitwiseOp::And,
            left: a,
            right: b,
        },
    );
    let mut facts = ScalarFacts::default();
    assert_eq!(facts.constant_across_selects(&table, both), None);
    let scope = facts.checkpoint();
    facts.assume(&table, at_least_twenty, true);
    assert_eq!(facts.constant_across_selects(&table, both), Some(1));
    let mut joined = facts.clone();
    joined.retain_common(&ScalarFacts::default());
    assert_eq!(joined.constant_across_selects(&table, both), None);
    facts.restore(scope);
    assert_eq!(facts.constant_across_selects(&table, both), None);
}

#[test]
fn predicates_can_require_more_than_one_shared_selector() {
    let mut table = ValueTable::default();
    let first = parameter(&mut table, Type::I1, 0);
    let second = parameter(&mut table, Type::I1, 1);
    let input = parameter(&mut table, Type::I64, 2);
    let a = correlated(&mut table, first, first, input);
    let b = correlated(&mut table, second, second, input);
    let either = expression(
        &mut table,
        Type::I1,
        Expression::Bitwise {
            operator: BitwiseOp::Or,
            left: a,
            right: b,
        },
    );
    assert_eq!(
        ScalarFacts::default().constant_across_selects(&table, either),
        Some(0)
    );
}

#[test]
fn exhausted_select_proofs_keep_the_original_predicate() {
    let mut table = ValueTable::default();
    let selector_input = parameter(&mut table, Type::I32, 0);
    let twenty = table.literal(Type::I32, 20);
    let condition = compare(&mut table, CompareOp::LtUnsigned, selector_input, twenty);
    let input = parameter(&mut table, Type::I64, 1);
    let invariant = correlated(&mut table, condition, condition, input);
    let mut facts = ScalarFacts::default();
    assert_eq!(
        facts.prove_select_cases(&table, invariant, &[condition], &mut 1),
        None
    );
    assert_eq!(facts.inferred_constant(&table, condition), None);
    let opposite = compare(&mut table, CompareOp::GeUnsigned, selector_input, twenty);
    assert_eq!(facts.constant(&table, opposite), None);
    let mut large = input;
    for offset in 1..=MAX_VALUES {
        let offset = table.literal(Type::I64, offset as u64);
        large = expression(
            &mut table,
            Type::I64,
            Expression::Binary {
                operator: BinaryOp::Add,
                left: large,
                right: offset,
            },
        );
    }
    let large_invariant = correlated(&mut table, condition, condition, large);
    assert!(facts.select_inputs(&table, large_invariant).is_none());
    assert_eq!(facts.constant_across_selects(&table, large_invariant), None);
}
