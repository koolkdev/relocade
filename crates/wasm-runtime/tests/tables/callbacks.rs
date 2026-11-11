use relocade_compiler::{abi, Program, I64, I8};
use relocade_wasm_runtime::{
    FunctionRef, Table, WasmBackend, WasmCompiler, WasmContext, WasmRuntime,
};
use std::{cell::RefCell, rc::Rc};

type Read = fn(u8) -> (u8, u64);

#[derive(Default)]
struct State {
    table: Option<Table<WasmRuntime, Read>>,
    events: Rc<RefCell<Vec<u32>>>,
}

async fn check_callback_replacement() {
    type Observations = fn() -> (u8, u8, u8, u64);
    const READ: abi::Table<Read> = abi::Table::new("test", "read", 1);
    const RECORD: abi::Table<fn(u8)> = abi::Table::new("test", "record", 1);
    const RUN: abi::Function<Observations> = abi::Function::new("test", "run");
    let state = Rc::new(RefCell::new(State::default()));
    let lifetime = Rc::downgrade(&state);
    let events = state.borrow().events.clone();
    let mut runtime = WasmRuntime::new().unwrap();
    let (run, _table) = {
        let captured = Rc::new(0x20_0000_0000_0001_u64);
        let owner = Rc::downgrade(&state);
        let initial =
            FunctionRef::<_, Read>::new(&mut runtime.context(), move |mut context, (value,)| {
                let owner = owner.upgrade().unwrap();
                let table = owner.borrow().table.as_ref().unwrap().clone();
                let events = owner.borrow().events.clone();
                let increment = value - 5;
                let replacement = FunctionRef::<WasmRuntime, Read>::new(
                    &mut context.reborrow(),
                    move |_, (value,)| {
                        events.borrow_mut().push(200 + u32::from(value));
                        (value + increment, 0x20_0000_0000_0003)
                    },
                );
                table.set(&mut context, 0, &replacement).unwrap();
                table
                    .ensure_size(&mut context, 2, Some(&replacement))
                    .unwrap();
                // Replacing our only slot must keep this invocation and its captures alive.
                owner
                    .borrow()
                    .events
                    .borrow_mut()
                    .push(100 + u32::from(value));
                (value + 1, *captured)
            });
        let recorded = events.clone();
        let record = FunctionRef::<_, fn(u8)>::new(&mut runtime.context(), move |_, (value,)| {
            recorded.borrow_mut().push(u32::from(value));
        });
        let table = runtime.create_table(READ, None).unwrap();
        table.set(&mut runtime.context(), 0, &initial).unwrap();
        let records = runtime.create_table(RECORD, Some(&record)).unwrap();
        state.borrow_mut().table = Some(table.clone());
        let mut program = Program::new();
        let read = program.import_table(READ);
        let record = program.import_table(RECORD);
        RUN.define(&mut program, move |mut body, ()| {
            let (first, before) = body.call_indirect::<(I8, I64)>(read, 0, &[7.into()])?;
            body.call_indirect::<()>(record, 0, &[(&first).into()])?;
            let (second, after) = body.call_indirect::<(I8, I64)>(read, 0, &[8.into()])?;
            let (third, grown) = body.call_indirect::<(I8, I64)>(read, 1, &[9.into()])?;
            body.return_((first, second, third, before.add(after).add(grown)))
        })
        .unwrap();
        let module = runtime
            .compiler()
            .compile(&program.compile().unwrap())
            .await
            .unwrap();
        let instance = runtime
            .instantiate(
                &module,
                &[runtime.bind(READ, &table), runtime.bind(RECORD, &records)],
            )
            .unwrap();
        (runtime.function(&instance, RUN).unwrap(), table)
    };
    assert_eq!(runtime.call(&run, ()), (8, 10, 11, 0x60_0000_0000_0007));
    assert_eq!(&*events.borrow(), &[107, 8, 208, 209]);
    events.borrow_mut().clear();
    assert_eq!(runtime.call(&run, ()), (9, 10, 11, 0x60_0000_0000_0009));
    assert_eq!(&*events.borrow(), &[207, 9, 208, 209]);
    // Retained functions must not keep their application owner alive through its table.
    drop(state);
    assert!(lifetime.upgrade().is_none());
    drop(runtime);
    // Native function handles do not own their store's captures. Browser engine
    // handles retain captures until engine collection.
    #[cfg(not(target_arch = "wasm32"))]
    assert_eq!(Rc::strong_count(&events), 1);
    drop(events);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_replace_themselves_and_grow_tables_without_relinking() {
    pollster::block_on(check_callback_replacement());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_replace_themselves_and_grow_tables_without_relinking() {
    check_callback_replacement().await;
}
