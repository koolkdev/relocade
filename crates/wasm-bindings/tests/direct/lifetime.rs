use super::{assert_values, forbid_js_calls, values, Scalars};
use js_sys::{Object, Reflect, Uint8Array, WebAssembly};
use relocade_wasm_bindings::{callback, Function};
use std::{cell::Cell, rc::Rc};
use wasm_bindgen::{prelude::wasm_bindgen, JsValue};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function as WasmFunction, FunctionSection,
    ImportSection, Instruction, Module, RefType, TableType, TypeSection, ValType,
};

struct Owner(Rc<Cell<bool>>);

impl Owner {
    fn check(&self) {
        assert!(!self.0.get());
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn table_references_keep_callbacks_alive_after_the_binding_is_dropped() {
    let descriptor = Object::new();
    Reflect::set(&descriptor, &"initial".into(), &1.into()).unwrap();
    Reflect::set(&descriptor, &"element".into(), &"anyfunc".into()).unwrap();
    let table = WebAssembly::Table::new(&descriptor).unwrap();
    let dropped = Rc::new(Cell::new(false));
    let calls = Rc::new(Cell::new(0));
    {
        let owner = Owner(Rc::clone(&dropped));
        let calls = Rc::clone(&calls);
        let target = callback::<Scalars, Scalars>(move |values| {
            owner.check();
            calls.set(calls.get() + 1);
            assert_values(values);
            values
        })
        .unwrap();
        forbid_js_calls(&target);
        let function = Function::<Scalars, Scalars>::new(&target).unwrap();
        table.set(0, function.as_function()).unwrap();
    }
    assert!(!dropped.get());
    let target = table.get(0).unwrap();
    let function = Function::<Scalars, Scalars>::new(&target).unwrap();
    drop(table);
    drop(target);
    for _ in 0..3 {
        assert_values(function.call(values()));
    }
    assert_eq!(calls.get(), 3);
}

#[wasm_bindgen(inline_js = "
export function collectNow() {
    globalThis.gc();
}
export async function collectAcrossTurns() {
    await new Promise(resolve => setTimeout(resolve, 0));
    globalThis.gc();
    await new Promise(resolve => setTimeout(resolve, 0));
}
")]
extern "C" {
    #[wasm_bindgen(js_name = collectNow)]
    fn collect_now();
    #[wasm_bindgen(js_name = collectAcrossTurns)]
    async fn collect_across_turns();
}

// Call through the table without creating a Rust handle to its callback. A
// binding obtained with table.get would itself keep that callback alive.
fn table_caller(table: &WebAssembly::Table) -> Function<(i32,), (i32,)> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    types.ty().function([], [ValType::I32]);
    types.ty().function([ValType::I32], [ValType::I32]);
    module.section(&types);
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "table",
        EntityType::Table(TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            minimum: 2,
            maximum: None,
            shared: false,
        }),
    );
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(1);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("call", ExportKind::Func, 0);
    module.section(&exports);
    let mut body = WasmFunction::new([]);
    body.instruction(&Instruction::LocalGet(0));
    body.instruction(&Instruction::CallIndirect {
        type_index: 0,
        table_index: 0,
    });
    body.instruction(&Instruction::End);
    let mut code = CodeSection::new();
    code.function(&body);
    module.section(&code);
    let module = WebAssembly::Module::new(&Uint8Array::from(module.finish().as_slice())).unwrap();
    let env = Object::new();
    Reflect::set(&env, &"table".into(), table).unwrap();
    let imports = Object::new();
    Reflect::set(&imports, &"env".into(), &env).unwrap();
    let instance = WebAssembly::Instance::new(&module, &imports).unwrap();
    Function::from_export(&instance, "call").unwrap()
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn nested_callbacks_release_their_owners_after_removing_the_last_table_references() {
    let descriptor = Object::new();
    Reflect::set(&descriptor, &"initial".into(), &2.into()).unwrap();
    Reflect::set(&descriptor, &"element".into(), &"anyfunc".into()).unwrap();
    let table = WebAssembly::Table::new(&descriptor).unwrap();
    let caller = Rc::new(table_caller(&table));
    let dropped = [Rc::new(Cell::new(false)), Rc::new(Cell::new(false))];
    for (index, dropped) in dropped.iter().enumerate() {
        let owner = Owner(Rc::clone(dropped));
        let callback_table = table.clone();
        let caller = Rc::clone(&caller);
        let target = callback::<(), (i32,)>(move |()| {
            owner.check();
            callback_table
                .set_raw(index as u32, &JsValue::NULL)
                .unwrap();
            collect_now();
            let value = if index == 0 {
                caller.call((1,)).0 + 23
            } else {
                17
            };
            collect_now();
            owner.check();
            (value,)
        })
        .unwrap();
        table.set(index as u32, &target).unwrap();
    }

    // Collection of an unreferenced callback is a control: the table must keep
    // both live callbacks intact across the same GC and finalizer turns.
    let control_dropped = Rc::new(Cell::new(false));
    let control = Owner(Rc::clone(&control_dropped));
    drop(callback::<(), ()>(move |()| control.check()).unwrap());
    for _ in 0..50 {
        collect_across_turns().await;
        assert!(dropped.iter().all(|flag| !flag.get()));
        if control_dropped.get() {
            break;
        }
    }
    assert!(
        control_dropped.get(),
        "unreferenced callback was not collected"
    );
    assert_eq!(caller.call((0,)), (40,));
    for _ in 0..50 {
        collect_across_turns().await;
        if dropped.iter().all(|flag| flag.get()) {
            break;
        }
    }
    assert!(
        dropped.iter().all(|flag| flag.get()),
        "removed callbacks were not collected"
    );
}
