use super::{Flag, FlagChange, FlagMask, FlagValues, StatusFlag};
use crate::alu::{AnyStatusSource, ArithmeticOp, StatusSource};
use wasm86_compiler::{Program, Signature, Type, I1, I16, I32};

#[test]
fn complete_status_masks_do_not_include_direction() {
    assert_eq!(Flag::ALL.len(), 7);
    assert_eq!(StatusFlag::ALL.len(), 6);
    for status in StatusFlag::ALL {
        assert!(FlagMask::STATUS.contains(status));
        assert!(FlagMask::ALL.contains(status));
    }
    assert!(!FlagMask::STATUS.contains(Flag::DF));
    assert!(FlagMask::ALL.contains(Flag::DF));
    assert!(FlagMask::STATUS.union(FlagMask::of(Flag::DF)) == FlagMask::ALL);
    assert!(FlagChange::partial([]).writes() == FlagMask::EMPTY);
}

#[test]
fn preserving_flags_keeps_the_arithmetic_source_and_its_width() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I16, Type::I16, Type::I1, Type::I1],
        results: vec![],
    });
    let body = program.define(function).unwrap();
    let left = body.parameter::<I16>(0).unwrap();
    let right = body.parameter::<I16>(1).unwrap();
    let first = body.parameter::<I1>(2).unwrap();
    let second = body.parameter::<I1>(3).unwrap();
    let outcome = ArithmeticOp::Subtract.apply(left.clone(), right.clone());
    assert!(outcome.flags.writes() == FlagMask::STATUS);
    let change = outcome
        .flags
        .when(first.clone())
        .preserving(StatusFlag::CF)
        .preserving(Flag::DF)
        .preserving(Flag::CF)
        .when(second.clone());
    for flag in Flag::ALL {
        assert_eq!(
            change.writes().contains(flag),
            flag != Flag::CF && flag != Flag::DF
        );
    }
    assert!(change.status_source(FlagMask::of(Flag::ZF)).is_some());
    assert!(change.status_source(FlagMask::of(Flag::CF)).is_none());
    assert!(change.status_source(FlagMask::of(Flag::DF)).is_none());
    assert!(change.status_source(FlagMask::STATUS).is_none());
    assert!(change
        .condition
        .as_ref()
        .unwrap()
        .same_expression(&first.and(second)));
    let FlagValues::Status(AnyStatusSource::Word(StatusSource::Arithmetic {
        operation,
        left: retained_left,
        right: retained_right,
        result,
    })) = &change.values
    else {
        panic!("preserving CF must retain the word arithmetic source");
    };
    assert!(*operation == ArithmeticOp::Subtract);
    assert!(retained_left.same_expression(&left));
    assert!(retained_right.same_expression(&right));
    assert!(result.same_expression(&outcome.result));
    body.return_(()).unwrap();
}

#[test]
fn conditioning_and_preserving_keep_explicit_values_and_the_write_mask() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I1; 7],
        results: vec![],
    });
    let body = program.define(function).unwrap();
    let values: [_; 7] = std::array::from_fn(|index| body.parameter::<I1>(index as u32).unwrap());
    let source = StatusSource::<I32>::Explicit {
        flags: std::array::from_fn(|index| values[index].clone()),
    };
    let complete = FlagChange::from(source)
        .when(values[0].clone())
        .preserving(Flag::CF)
        .when(values[5].clone());
    assert!(!complete.writes().contains(Flag::CF));
    assert!(!complete.writes().contains(Flag::DF));
    for flag in StatusFlag::ALL.into_iter().skip(1) {
        assert!(complete.writes().contains(flag));
    }
    let FlagValues::Status(AnyStatusSource::Dword(StatusSource::Explicit { flags })) =
        &complete.values
    else {
        panic!("masking a status source must retain its explicit status values");
    };
    for (flag, original) in flags.iter().zip(&values) {
        assert!(flag.same_expression(original));
    }

    let partial = FlagChange::partial([
        (Flag::CF, values[0].clone()),
        (Flag::OF, values[5].clone()),
        (Flag::DF, values[6].clone()),
    ])
    .when(values[2].clone())
    .preserving(StatusFlag::CF)
    .preserving(Flag::PF)
    .when(values[3].clone());
    assert!(!partial.writes().contains(Flag::CF));
    assert!(partial.writes() == FlagMask::of(Flag::OF).union(FlagMask::of(Flag::DF)));
    assert!(partial
        .condition
        .as_ref()
        .unwrap()
        .same_expression(&values[2].and(&values[3])));
    let FlagValues::Explicit(flags) = &partial.values else {
        panic!("explicit common flags must retain their provided logical values");
    };
    for flag in [Flag::OF, Flag::DF] {
        assert!(flags[flag.index()]
            .as_ref()
            .unwrap()
            .same_expression(&values[flag.index()]));
    }
    assert!(partial.preserving(Flag::OF).preserving(Flag::DF).writes() == FlagMask::EMPTY);
    body.return_(()).unwrap();
}
