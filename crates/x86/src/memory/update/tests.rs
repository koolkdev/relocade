use std::sync::Barrier;

use super::{native_update, Intent, Memory, OperandUpdate};
use wasm86_compiler::{MemoryImport, Program, Signature, Type, I32, I64};
use wasm86_test_support::{engine, Module, SharedBytes};
use wasmparser::{Operator, Parser, Payload};

#[test]
fn aligned_complete_updates_need_no_scattered_access_helpers() {
    let mut program = Program::new();
    let memory = Memory::declare(&mut program).unwrap();
    let update = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
            |mut body| {
                let access = memory.resolve_access(
                    &mut body,
                    &0x4000.into(),
                    8,
                    Intent::Write,
                    crate::state::exit::exception,
                )?;
                let previous = memory.atomic_update(
                    &mut body,
                    &access,
                    &OperandUpdate::<I64>::Exchange(7.into()),
                )?;
                body.return_(previous)
            },
        )
        .unwrap();
    program.export("update", update).unwrap();
    let bytes = program.compile().unwrap();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for operator in body.get_operators_reader().unwrap() {
                assert!(!matches!(operator.unwrap(), Operator::Call { .. }));
            }
        }
    }
}

#[test]
fn concurrent_negations_return_each_value_they_replace() {
    let mut program = Program::new();
    let guest = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "guest".into(),
        minimum: 1,
        maximum: Some(1),
        shared: true,
    });
    let negate = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I32],
            },
            |mut body| {
                let previous =
                    native_update::<I32>(&mut body, guest, &0.into(), &OperandUpdate::Negate)?;
                body.return_(previous)
            },
        )
        .unwrap();
    program.export("negate", negate).unwrap();
    let module = Module::new(&program.compile().unwrap());
    let shared = SharedBytes::new(1, 1);
    shared.write(0, &7u32.to_le_bytes());
    let ready = Barrier::new(4);
    let mut observed = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let mut store = wasmtime::Store::new(engine(), ());
                    let mut linker = wasmtime::Linker::new(engine());
                    linker
                        .define(&store, "test", "guest", shared.memory().clone())
                        .unwrap();
                    let instance = linker.instantiate(&mut store, module.wasmtime()).unwrap();
                    let negate = instance
                        .get_typed_func::<(), i32>(&mut store, "negate")
                        .unwrap();
                    ready.wait();
                    (0..8193)
                        .map(|_| negate.call(&mut store, ()).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    observed.sort_unstable();
    let half = observed.len() / 2;
    assert_eq!(&observed[..half], vec![-7; half]);
    assert_eq!(&observed[half..], vec![7; half]);
    assert_eq!(shared.read(0, 4), 7u32.to_le_bytes());
}
