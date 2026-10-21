use crate::backend;
use relocade_runtime::{wasm::WasmBackend, Error};
use std::{cell::Cell, rc::Rc};

thread_local! {
    static CALLER: Cell<bool> = const { Cell::new(false) };
    static REQUESTS: Cell<u8> = const { Cell::new(0) };
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn worker_compilation() -> Result<(), String> {
    CALLER.set(true);
    let mut first = backend(Rc::new(Cell::new(7))).map_err(|error| error.to_string())?;
    let second = backend(Rc::new(Cell::new(9))).map_err(|error| error.to_string())?;
    let error = first
        .compile_with(|| {
            assert!(!CALLER.get(), "lowering must run on the compiler worker");
            REQUESTS.set(1);
            Err(Error::wasm("lower test", "failed"))
        })
        .await
        .expect_err("lowering must fail");
    assert!(error.to_string().contains("lower test"));

    let error = second
        .compile_with(|| {
            assert!(!CALLER.get());
            assert_eq!(REQUESTS.replace(2), 1, "backends must reuse the worker");
            Ok(b"invalid Wasm".to_vec())
        })
        .await
        .expect_err("compilation must fail");
    assert!(error.to_string().contains("compile Wasm"));

    let module = first
        .compile_with(|| {
            assert!(!CALLER.get());
            assert_eq!(
                REQUESTS.replace(3),
                2,
                "errors must leave the worker usable"
            );
            Ok(b"\0asm\x01\0\0\0".to_vec())
        })
        .await
        .map_err(|error| error.to_string())?;
    first
        .instantiate(&module, &[])
        .await
        .map_err(|error| error.to_string())?;
    assert_eq!(first.state().get(), 7);
    assert_eq!(second.state().get(), 9);
    assert!(CALLER.get());
    Ok(())
}
