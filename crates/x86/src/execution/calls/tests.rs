use super::*;
use crate::{ExecutionExit, HostExit};
use relocade_compiler::abi::{
    slots::{Layout, Tuple},
    AbiValue,
};
use relocade_test_support::Module;
use wasmtime::{Caller, Func, Linker, Ref, RefType, Store, Table as WasmTable, TableType};

fn result(value: u32) -> <Layout<ExecutionExit> as Tuple>::Tuple {
    Layout::<ExecutionExit>::into_tuple(ExecutionExit::lower(ExecutionExit::Host(HostExit::Call {
        service_id: value,
    })))
}

#[test]
fn constant_call_slots_need_no_lookup_or_cache_and_follow_external_updates() {
    let mut program = Program::new();
    let table = program.import_table(abi::CALL_TARGETS);
    let call = program.function::<abi::ExecutionSignature>(move |body, ()| {
        CallLink { table, index: 1 }.enter(body)
    });
    program.export("call", call).unwrap();
    let module = Module::new(&program.compile().unwrap());
    let imports: Vec<_> = module
        .wasmtime()
        .imports()
        .map(|import| (import.module().to_owned(), import.name().to_owned()))
        .collect();
    assert_eq!(imports, [("relocade".into(), "callTargets".into())]);
    let engine = relocade_test_support::engine();
    let mut store = Store::new(engine, 0u32);
    let unresolved = Func::wrap(&mut store, |mut caller: Caller<'_, u32>| {
        *caller.data_mut() += 1;
        result(1)
    });
    let table = WasmTable::new(
        &mut store,
        TableType::new(RefType::FUNCREF, 2, None),
        Ref::Func(Some(unresolved)),
    )
    .unwrap();
    let mut linker = Linker::new(engine);
    linker
        .define(&store, "relocade", "callTargets", table)
        .unwrap();
    let instance = linker.instantiate(&mut store, module.wasmtime()).unwrap();
    let call = instance
        .get_typed_func::<(), <Layout<ExecutionExit> as Tuple>::Tuple>(&mut store, "call")
        .unwrap();
    for count in 1..=2 {
        assert_eq!(call.call(&mut store, ()).unwrap(), result(1));
        assert_eq!(*store.data(), count);
    }
    // Installation updates the existing caller's constant slot; subsequent calls
    // do not invoke the unresolved destination, even on the first linked call.
    for value in [2, 3] {
        let target = Func::wrap(&mut store, move || result(value));
        table.set(&mut store, 1, Ref::Func(Some(target))).unwrap();
        assert_eq!(call.call(&mut store, ()).unwrap(), result(value));
        assert_eq!(*store.data(), 2);
    }
}
