use relocade_compiler::{abi, Program, I64, I8};
use relocade_wasm_runtime::{Backend, FunctionRef, Table, WasmBackend, WasmCompiler, WasmContext};
use std::{cell::RefCell, rc::Rc};

type Read = fn(u8) -> (u8, u64);

#[derive(Default)]
struct State {
    table: Option<Table<Backend<Self>, Read>>,
    events: Rc<RefCell<Vec<u32>>>,
    lifetime: Rc<()>,
}

async fn check_callback_replacement() {
    type Observations = fn() -> (u8, u8, u8, u64);
    const READ: abi::Table<Read> = abi::Table::new("test", "read", 1);
    const RECORD: abi::Table<fn(u8)> = abi::Table::new("test", "record", 1);
    const RUN: abi::Function<Observations> = abi::Function::new("test", "run");
    let state = State::default();
    let lifetime = Rc::downgrade(&state.lifetime);
    let events = state.events.clone();
    let mut backend = Backend::with_state(state).unwrap();
    let (run, _table) = {
        let captured = Rc::new(0x20_0000_0000_0001_u64);
        let initial =
            FunctionRef::<_, Read>::new(&mut backend.context(), move |mut context, (value,)| {
                let state = context.state();
                let table = state.table.as_ref().unwrap().clone();
                let events = state.events.clone();
                let increment = value - 5;
                let replacement = FunctionRef::<Backend<State>, Read>::new(
                    &mut context.table_context(),
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
                context
                    .state()
                    .events
                    .borrow_mut()
                    .push(100 + u32::from(value));
                (value + 1, *captured)
            });
        let record = FunctionRef::<_, fn(u8)>::new(&mut backend.context(), |context, (value,)| {
            context.state().events.borrow_mut().push(u32::from(value));
        });
        let table = backend.create_table(READ, None).unwrap();
        table.set(&mut backend.context(), 0, &initial).unwrap();
        let records = backend.create_table(RECORD, Some(&record)).unwrap();
        backend.state_mut().table = Some(table.clone());
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
        let module = backend
            .compiler()
            .compile(&program.compile().unwrap())
            .await
            .unwrap();
        let instance = backend
            .instantiate(
                &module,
                &[backend.bind(READ, &table), backend.bind(RECORD, &records)],
            )
            .await
            .unwrap();
        (backend.function(&instance, RUN).unwrap(), table)
    };
    assert_eq!(backend.call(&run, ()), (8, 10, 11, 0x60_0000_0000_0007));
    assert_eq!(&*backend.state().events.borrow(), &[107, 8, 208, 209]);
    backend.state().events.borrow_mut().clear();
    assert_eq!(backend.call(&run, ()), (9, 10, 11, 0x60_0000_0000_0009));
    assert_eq!(&*backend.state().events.borrow(), &[207, 9, 208, 209]);
    drop(backend);
    // Retained tables, instances and callbacks must not form a cycle through local state.
    assert!(lifetime.upgrade().is_none());
    // Native function handles do not own their store's captures. Browser engine
    // handles retain captures until collection, independently of backend state.
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

async fn check_table_context_replacement() {
    type Read = fn(u8) -> u8;
    const READ: abi::Table<Read> = abi::Table::new("test", "read", 1);
    const RUN: abi::Function<fn() -> (u8, u8)> = abi::Function::new("test", "run");
    let mut backend = Backend::new().unwrap();
    let table = backend.create_table(READ, None).unwrap();
    let target = table.clone();
    let captures = Rc::new(3_u8);
    let initial = FunctionRef::<Backend, Read>::new(
        &mut backend.table_context(),
        move |mut context, (value,)| {
            let increment = *captures;
            let replacement =
                FunctionRef::<Backend, Read>::new(&mut context, move |_, (value,)| {
                    value + increment
                });
            target.set(&mut context, 0, &replacement).unwrap();
            value + 1
        },
    );
    table.set(&mut backend, 0, &initial).unwrap();
    let mut program = Program::new();
    let read = program.import_table(READ);
    RUN.define(&mut program, move |mut body, ()| {
        let first = body.call_indirect::<I8>(read, 0, &[7.into()])?;
        let second = body.call_indirect::<I8>(read, 0, &[9.into()])?;
        body.return_((first, second))
    })
    .unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let instance = backend
        .instantiate(&module, &[backend.bind(READ, &table)])
        .await
        .unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    assert_eq!(backend.call(&run, ()), (8, 12));
    assert_eq!(backend.call(&run, ()), (10, 12));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn table_context_prepares_and_publishes_callback_replacement() {
    pollster::block_on(check_table_context_replacement());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn table_context_prepares_and_publishes_callback_replacement() {
    check_table_context_replacement().await;
}
