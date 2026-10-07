//! Only functions reachable from exports after specialization belong in the module.

use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, Observation, TestModule, Value};
use wasm86_compiler::{FunctionImport, MemoryImport, Type, I1, I32};
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
    let leaf = fixture
        .program
        .function(signature(&[], &[]), |mut body| {
            body.store::<I32>(memory, 0, 99)?;
            body.call::<()>(called, &[])?;
            body.return_(())
        })
        .unwrap();
    let helper = fixture
        .program
        .function(signature(&[], &[]), |mut body| {
            body.call::<()>(leaf, &[])?;
            body.return_(())
        })
        .unwrap();
    let recursive = fixture.program.declare(signature(&[], &[]));
    fixture
        .program
        .define(recursive, |body| body.tail_call(recursive, &[]))
        .unwrap();
    fixture.function(&[Type::I1], &[Type::I32], |mut body| {
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
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn removed_helpers_need_no_host_imports_in_v8() {
    for (condition, expected) in [(0, 0), (1, 7)] {
        assert_eq!(
            removed_helpers().run_v8(&Input::call("run", &[Value::I32(condition)])),
            Observation::returned(&[Value::I32(expected)])
        );
    }
}
