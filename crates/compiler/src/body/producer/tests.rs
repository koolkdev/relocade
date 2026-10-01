use super::*;
use crate::{FunctionImport, FunctionKind, Program, Signature, Type, I32, I64};

#[test]
fn producer_views_preserve_dependencies_and_zero_one_or_many_results() {
    let mut program = Program::new();
    let callee = program.import_function(FunctionImport {
        module: "host".into(),
        name: "pair".into(),
        signature: Signature {
            parameters: vec![Type::I32],
            results: vec![Type::I32, Type::I64],
        },
    });
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I32, Type::I64],
            },
            |mut body| {
                let input = body.parameter::<I32>(0)?;
                let results = body.call::<(I32, I64)>(callee, &[input.add(7).argument()])?;
                body.atomic_fence();
                body.return_(results)
            },
        )
        .unwrap();
    let FunctionKind::Defined(Some(graph)) = &program.functions[function.0].kind else {
        panic!("the function has a completed graph");
    };
    let &[call, fence] = graph.blocks[graph.entry.0].items.as_slice() else {
        panic!("construction placed the call and fence");
    };
    let results = graph.results(call).collect::<Vec<_>>();
    let types: Vec<_> = results
        .iter()
        .map(|&value| graph.values[value].ty)
        .collect();
    assert_eq!(types, [Type::I32, Type::I64]);
    assert!(results
        .iter()
        .all(|&value| graph.producer_of(value) == Some(call)));

    let arguments: Vec<_> = graph.inputs(call).collect();
    assert_eq!(arguments.len(), 1);
    let sum = arguments[0];
    let calculation = graph.producer_of(sum).unwrap();
    assert!(matches!(calculation, BlockItem::Evaluate(value) if value == sum));
    assert_eq!(graph.results(calculation).collect::<Vec<_>>(), [sum]);
    let inputs: Vec<_> = graph.inputs(calculation).collect();
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0], graph.blocks[graph.entry.0].parameters[0]);
    assert!(matches!(
        graph.values[inputs[1]].definition,
        ValueDefinition::Constant(7)
    ));
    assert_eq!(
        graph.inputs(calculation).rev().collect::<Vec<_>>(),
        [inputs[1], inputs[0]]
    );
    assert!(inputs
        .iter()
        .all(|&value| graph.producer_of(value).is_none()));

    assert!(graph.results(fence).next().is_none());
    assert!(graph.inputs(fence).next().is_none());
}

#[test]
fn wide_components_expose_one_producer_with_ordered_results() {
    let mut program = Program::new();
    let function = program
        .function(
            Signature {
                parameters: vec![Type::I64; 2],
                results: vec![Type::I64; 2],
            },
            |body| {
                let (low, high) = body
                    .parameter::<I64>(0)?
                    .unsigned()
                    .mul_wide(body.parameter::<I64>(1)?);
                body.return_((high, low))
            },
        )
        .unwrap();
    let FunctionKind::Defined(Some(graph)) = &program.functions[function.0].kind else {
        panic!("the function has a completed graph")
    };
    let super::super::Exit::Return(results) = &graph.blocks[0].exit else {
        panic!("the fixture returns both components")
    };
    let producer = graph.producer_of(results[0]).unwrap();
    assert!(graph.producer_of(results[1]) == Some(producer));
    assert_eq!(
        graph.results(producer).collect::<Vec<_>>(),
        [results[1], results[0]]
    );
    assert_eq!(
        graph.inputs(producer).collect::<Vec<_>>(),
        graph.blocks[0].parameters
    );
    for (component, &id) in results.iter().rev().enumerate() {
        let view = graph.values.expression(id).unwrap();
        assert_eq!(view.component, component);
        assert!(matches!(
            view.expression,
            Expression::MultiplyWide { signed: false, .. }
        ));
    }
}
