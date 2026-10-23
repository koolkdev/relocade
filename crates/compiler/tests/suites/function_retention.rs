//! Only functions reachable from exports after specialization belong in the module.

use crate::fixture::{signature, Fixture};
use crate::wasm::TestModule;
use relocade_compiler::{FunctionImport, MemoryImport, Type, I1, I32};
use wasmparser::{Parser, Payload};

fn removed_helpers() -> TestModule {
    let mut fixture = Fixture::new();
    // No host definitions are supplied for these imports. The removed helper
    // chain is their only consumer.
    let memory = fixture.program.import_memory(MemoryImport {
        module: "test".into(),
        name: "dead_memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let called = fixture.program.import_function(FunctionImport {
        module: "test".into(),
        name: "dead_call".into(),
        signature: signature(&[], &[]),
    });
    let leaf = fixture.program.function::<fn()>(move |mut body, ()| {
        body.store::<I32>(memory, 0, 99)?;
        body.call::<()>(called, &[])?;
        body.return_(())
    });
    let helper = fixture.program.function::<fn()>(move |mut body, ()| {
        body.call::<()>(leaf, &[])?;
        body.return_(())
    });
    let recursive = fixture.program.declare(signature(&[], &[]));
    fixture
        .program
        .define(recursive, move |body| body.tail_call(recursive, &[]))
        .unwrap();
    fixture.function(&[Type::I1], &[Type::I32], move |mut body| {
        let condition = body.parameter::<I1>(0)?;
        body.if_(condition.eq(false), |arm| arm.return_(0))?;
        let result = body.if_value::<I32>(
            condition,
            |arm| arm.yield_(7),
            |mut arm| {
                arm.call::<()>(helper, &[])?;
                arm.yield_(99)
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn removed_helpers_do_not_retain_functions_or_imports() {
    let module = removed_helpers();
    let mut functions = 0;
    for payload in Parser::new(0).parse_all(module.bytes()) {
        match payload.unwrap() {
            Payload::CodeSectionStart { count, .. } => functions = count,
            Payload::ImportSection(imports) => assert_eq!(imports.count(), 0),
            _ => {}
        }
    }
    assert_eq!(functions, 1);
    assert_eq!(module.instantiate().call::<i32>(1), Ok(7));
    assert_eq!(module.instantiate().call::<i32>(0), Ok(0));
}

#[test]
fn bodies_are_built_once_on_reachable_calls_before_placement() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    let mut fixture = Fixture::new();
    let built = Arc::new(AtomicUsize::new(0));
    let builds = Arc::clone(&built);
    let helper = fixture.program.function::<fn() -> u32>(move |body, ()| {
        builds.fetch_add(1, Ordering::Relaxed);
        body.return_(7)
    });
    let unused = fixture
        .program
        .function::<fn() -> u32>(move |_, ()| panic!("unreachable helper must stay unbuilt"));
    fixture
        .program
        .function::<fn() -> u32>(move |body, ()| body.tail_call(unused, &[]));
    let run = fixture
        .program
        .function::<fn() -> u32>(move |mut body, ()| {
            body.if_(false, |mut arm| {
                arm.call::<I32>(unused, &[])?;
                arm.tail_call(unused, &[])
            })?;
            let first = body.call::<I32>(helper, &[])?;
            let second = body.call::<I32>(helper, &[])?;
            body.return_(first.add(second))
        });
    // Exporting the shared helper adds another construction root.
    fixture.program.export("helper", helper).unwrap();
    assert_eq!(built.load(Ordering::Relaxed), 0);
    let module = fixture.finish(run);
    assert_eq!(built.load(Ordering::Relaxed), 1);
    assert_eq!(module.instantiate().call::<i32>(()), Ok(14));
}

#[test]
fn roots_can_define_nested_call_and_tail_helpers() {
    let mut fixture = Fixture::new();
    let run = fixture
        .program
        .function::<fn() -> u32>(move |mut body, ()| {
            let helper = body.program().function::<fn() -> u32>(move |mut body, ()| {
                let leaf = body
                    .program()
                    .function::<fn() -> u32>(move |body, ()| body.return_(23));
                body.tail_call(leaf, &[])
            });
            let value = body.call::<I32>(helper, &[])?;
            body.return_(value.add(1))
        });
    assert_eq!(fixture.finish(run).instantiate().call::<i32>(()), Ok(24));
}

#[test]
fn mutual_recursion_terminates_construction() {
    let mut fixture = Fixture::new();
    let run = fixture
        .program
        .declare(signature(&[Type::I32], &[Type::I32]));
    let helper = fixture
        .program
        .raw_function(signature(&[Type::I32], &[Type::I32]), move |body| {
            let remaining = body.parameter::<I32>(0)?;
            body.tail_call(run, &[remaining.into()])
        });
    fixture
        .program
        .define(run, move |mut body| {
            let remaining = body.parameter::<I32>(0)?;
            body.if_(remaining.eq(0), |arm| arm.return_(9))?;
            body.tail_call(helper, &[remaining.sub(1).into()])
        })
        .unwrap();
    assert_eq!(fixture.finish(run).instantiate().call::<i32>(4), Ok(9));
}
