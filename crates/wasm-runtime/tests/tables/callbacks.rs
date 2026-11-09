use relocade_compiler::{abi, Program, I64, I8};
use relocade_wasm_runtime::{Backend, FunctionRef, Table, WasmBackend, WasmCompiler, WasmContext};
use std::{cell::RefCell, rc::Rc};

type Read = fn(u8) -> (u8, u64);

#[derive(Default)]
struct State {
    table: Option<Table<Backend<Self>, Read>>,
    events: Rc<RefCell<Vec<u32>>>,
}

async fn check_callback_replacement() {
    type Observations = fn() -> (u8, u8, u8, u64);
    const READ: abi::Table<Read> = abi::Table::new("test", "read", 1);
    const RECORD: abi::Table<fn(u8)> = abi::Table::new("test", "record", 1);
    const RUN: abi::Function<Observations> = abi::Function::new("test", "run");
    let state = State::default();
    let lifetime = Rc::downgrade(&state.events);
    let mut backend = Backend::with_state(state).unwrap();
    let (run, _table) = {
        let captured = Box::new(0x20_0000_0000_0001_u64);
        let initial = backend.host_function::<Read>(Box::new(move |mut context, value| {
            let state = context.state();
            let table = state.table.as_ref().unwrap().clone();
            let increment = value - 5;
            let replacement = FunctionRef::<Backend<State>, Read>::host(
                &mut context,
                Box::new(move |context, value| {
                    context
                        .state()
                        .events
                        .borrow_mut()
                        .push(200 + u32::from(value));
                    (value + increment, 0x20_0000_0000_0003)
                }),
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
        }));
        let record = backend.host_function::<fn(u8)>(Box::new(|context, value| {
            context.state().events.borrow_mut().push(u32::from(value));
        }));
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
