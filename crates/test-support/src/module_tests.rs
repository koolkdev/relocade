use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
};

use serde_json::{json, Value};

use super::Module;

// A mutable i32 global starts at zero; run(delta) adds delta and returns it.
const COUNTER: &[u8] = b"\0asm\x01\0\0\0\x01\x06\x01\x60\x01\x7f\x01\x7f\x03\x02\x01\0\x06\x06\x01\x7f\x01\x41\0\x0b\x07\x07\x01\x03run\0\0\x0a\x0d\x01\x0b\0\x23\0\x20\0\x6a\x24\0\x23\0\x0b";

fn run(module: &Module, input: Value) -> Value {
    module.run_v8(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/v8/test-adapter.mjs"),
        &input,
    )
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_module_lifecycle_keeps_requests_independent() {
    let first = Module::new(COUNTER);
    let mut different_counter = COUNTER.to_vec();
    different_counter[26] = 20; // The global's i32.const initializer.
    let second = Module::new(&different_counter);
    for (module, delta, result, reused) in [
        (&first, 3, 3, false),
        (&first, 7, 7, true),
        (&second, 11, 31, false),
    ] {
        assert_eq!(
            run(module, json!({ "delta": delta })),
            json!({ "result": result, "reused": reused })
        );
    }
    // V8 requests only read the module bytes and ID; its Wasmtime slot is unused.
    let error = catch_unwind(AssertUnwindSafe(|| {
        run(&first, json!({ "error": "adapter failure\nwith details" }))
    }))
    .unwrap_err();
    assert!(error
        .downcast_ref::<String>()
        .unwrap()
        .contains("adapter failure\nwith details"));
    assert_eq!(
        run(&first, json!({ "delta": 13 })),
        json!({ "result": 13, "reused": true })
    );
    assert!(catch_unwind(AssertUnwindSafe(|| run(&first, json!({ "exit": true })))).is_err());
    assert_eq!(
        run(&first, json!({ "delta": 2 })),
        json!({ "result": 2, "reused": false })
    );
}
