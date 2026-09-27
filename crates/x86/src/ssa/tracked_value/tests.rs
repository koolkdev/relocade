use super::TrackedValue;
use wasm86_compiler::{Program, Signature, Type, I16, I32};

#[test]
fn only_new_expressions_change_the_current_definition() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
    });
    program
        .define(function, |mut body| {
            let input = body.parameter::<I32>(0).unwrap();
            let mut tracked = TrackedValue::new(&mut body, &input).unwrap();
            let held = tracked.read(&mut body).unwrap();
            assert!(tracked.dirty_value().is_none());
            assert!(!tracked.define(&mut body, input.add(0)).unwrap());
            assert!(tracked.dirty_value().is_none());

            assert!(tracked.define(&mut body, input.add(7)).unwrap());
            let changed = tracked.read(&mut body).unwrap();
            assert!(tracked.dirty_value().unwrap().same_expression(&changed));
            assert!(!tracked.define(&mut body, &changed).unwrap());
            assert!(tracked.dirty_value().is_some());
            assert!(held.same_expression(&input));
            assert!(!held.same_expression(&changed));

            tracked.mark_clean();
            assert!(tracked.dirty_value().is_none());
            assert!(tracked.read(&mut body).unwrap().same_expression(&changed));
            body.return_(changed)
        })
        .unwrap();
    program.compile().unwrap();
}

#[test]
fn forked_definitions_keep_independent_values_and_publication_state() {
    let mut program = Program::new();
    let function = program.declare(Signature {
        parameters: vec![],
        results: vec![Type::I16],
    });
    program
        .define(function, |mut body| {
            let mut original = TrackedValue::<I16>::defined(&mut body, 11).unwrap();
            let mut fork = original.clone();
            let before = original.read(&mut body).unwrap();
            fork.mark_clean();
            assert!(fork.dirty_value().is_none());
            assert!(original.dirty_value().is_some());
            original.define(&mut body, 13).unwrap();
            assert!(fork.read(&mut body).unwrap().same_expression(&before));
            assert!(original
                .read(&mut body)
                .unwrap()
                .same_expression(&body.value::<I16>(13).unwrap()));
            assert!(original.dirty_value().is_some());
            assert!(fork.dirty_value().is_none());
            body.return_(original.value())
        })
        .unwrap();
    program.compile().unwrap();
}
