use crate::{BuildError, IntType, Program, Signature, Type, Val, I1, I16, I32, I64, I8};

#[test]
fn uniform_constant_choices_fold_comparisons_in_both_operand_orders() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32, Type::I32],
        results: vec![Type::I1],
    });
    let body = program.define(function).unwrap();
    let first = body.parameter::<I32>(0).unwrap().eq(0);
    let second = body.parameter::<I32>(1).unwrap().eq(0);
    let choice = first.select::<I16>(0, second.select::<I16>(1, 2));
    for (result, expected) in [
        (choice.eq(3), false),
        (choice.ne(3), true),
        (choice.unsigned().lt(3), true),
        (choice.unsigned().ge(3), false),
        (Val::<I16>::from(3).eq(&choice), false),
        (Val::<I16>::from(3).ne(&choice), true),
        (Val::<I16>::from(3).unsigned().lt(&choice), false),
        (Val::<I16>::from(3).unsigned().ge(&choice), true),
    ] {
        assert!(result.same_expression(&body.value::<I1>(expected).unwrap()));
    }
    body.return_(choice.eq(3)).unwrap();
    program.compile().unwrap();
}

#[test]
fn constant_choice_comparisons_use_the_logical_sign() {
    fn check<T: IntType>() {
        let mut program = Program::new();
        let function = program.declare(Signature {
            parameters: vec![Type::I32],
            results: vec![Type::I1],
        });
        let body = program.define(function).unwrap();
        let condition = body.parameter::<I32>(0).unwrap().eq(0);
        let high_bit = Val::<T>::literal(1_u64 << (T::TYPE.bits() - 1));
        let minus_one = Val::<T>::literal(T::TYPE.mask());
        let choice = condition.select(high_bit, minus_one);
        for (result, expected) in [
            (choice.signed().lt(0), true),
            (choice.signed().ge(0), false),
            (choice.unsigned().lt(0), false),
            (Val::<T>::from(0).signed().lt(&choice), false),
            (Val::<T>::from(0).signed().ge(&choice), true),
            (Val::<T>::from(0).unsigned().lt(&choice), true),
        ] {
            assert!(result.same_expression(&body.value::<I1>(expected).unwrap()));
        }
        body.return_(choice.signed().lt(0)).unwrap();
        program.compile().unwrap();
    }
    check::<I8>();
    check::<I16>();
    check::<I32>();
    check::<I64>();
}

#[test]
fn mixed_or_unknown_choices_keep_a_runtime_comparison() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32, Type::I32],
        results: vec![Type::I1, Type::I1],
    });
    let body = program.define(function).unwrap();
    let condition = body.parameter::<I32>(0).unwrap().eq(0);
    let unknown = body.parameter::<I32>(1).unwrap();
    let mixed = condition.select::<I32>(0, 3).eq(3);
    let unproved = condition.select(0, unknown).unsigned().lt(3);
    for result in [&mixed, &unproved] {
        for constant in [false, true] {
            assert!(!result.same_expression(&body.value::<I1>(constant).unwrap()));
        }
    }
    body.return_((mixed, unproved)).unwrap();
    program.export("run", function).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<(i32, i32), (i32, i32)>(&mut store, "run")
        .unwrap();
    for (input, expected) in [((0, 5), (0, 1)), ((1, 5), (1, 0)), ((1, 2), (1, 1))] {
        assert_eq!(run.call(&mut store, input).unwrap(), expected);
    }
}

#[test]
fn folded_choice_comparisons_retain_the_conditions_scope() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I1],
    });
    let mut body = program.define(function).unwrap();
    let input = body.parameter::<I32>(0).unwrap();
    let mut comparison = None;
    body.if_(true, |mut child| {
        let condition =
            child.if_value::<I1>(input.eq(0), |arm| arm.yield_(true), |arm| arm.yield_(false))?;
        let folded = condition.select::<I16>(0, 1).eq(3);
        assert!(folded.same_expression(&child.value::<I1>(false)?));
        comparison = Some(folded);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        body.value(comparison.unwrap()).err(),
        Some(BuildError::OutOfScope)
    );
    body.return_(false).unwrap();
    program.compile().unwrap();
}
