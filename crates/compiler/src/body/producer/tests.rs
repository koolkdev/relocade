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
    let results = graph.results(&call);
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
    assert_eq!(graph.results(&calculation), [sum]);
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

    assert!(graph.results(&fence).is_empty());
    assert!(graph.inputs(fence).next().is_none());
}
