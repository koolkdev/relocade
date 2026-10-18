#![cfg(not(target_arch = "wasm32"))]

use wasm86_compiler::{
    abi::{Function, Memory, Table},
    Program, I32, I64,
};
use wasm86_runtime::wasm::{WasmBackend, WasmMemoryAccess};
use wasm86_runtime_wasmtime::WasmtimeBackend;

const SHARED: Memory = Memory::new("example", "shared", 1);
const FUNCTIONS: Table<fn() -> I64> = Table::new("example", "functions", 1);
const ADVANCE: Function<fn() -> I64> = Function::new("example", "advance");
const NEXT: Function<fn() -> I64> = Function::new("example", "next");
const WIDE: Function<fn(I32, I32) -> I64> = Function::new("example", "wide");
const EXECUTE: Function<fn() -> I64> = Function::new("example", "execute");

#[test]
fn modules_share_live_memory_and_functions_after_linking() {
    pollster::block_on(async {
        let mut backend = WasmtimeBackend::new().unwrap();
        let memory = backend.create_memory(1, None).unwrap();
        let table = backend.create_table(1).unwrap();
        let entry = {
            let mut producer = Program::new();
            let shared = producer.import_memory(SHARED);
            let advance = producer
                .function(ADVANCE.signature(), |mut body| {
                    let next = body.load::<I64>(shared, 0)?.add(1);
                    body.store(shared, 0, &next)?;
                    body.return_(next)
                })
                .unwrap();
            ADVANCE.export(&mut producer, advance).unwrap();
            let module = backend.compile(&producer.compile().unwrap()).await.unwrap();
            let instance = backend
                .instantiate(&module, &[backend.bind(SHARED, &memory)])
                .await
                .unwrap();
            let advance = backend.function(&instance, ADVANCE).unwrap();
            backend.set_table_function(&table, 0, &advance).unwrap();

            let mut consumer = Program::new();
            let shared = consumer.import_memory(SHARED);
            let functions = consumer.import_table(FUNCTIONS);
            let direct = consumer.import_function(NEXT);
            let wide = consumer.import_function(WIDE);
            let execute = consumer
                .function(EXECUTE.signature(), |mut body| {
                    let increment = body.call::<I64>(wide, &[1.into(), 3.into()])?;
                    let current = body.load::<I64>(shared, 0)?;
                    body.store(shared, 0, current.add(increment))?;
                    body.call::<I64>(direct, &[])?;
                    body.tail_call_indirect(functions, 0, &[])
                })
                .unwrap();
            EXECUTE.export(&mut consumer, execute).unwrap();
            let module = backend.compile(&consumer.compile().unwrap()).await.unwrap();
            let imports = [
                backend.bind(SHARED, &memory),
                backend.bind(FUNCTIONS, &table),
                backend.bind(NEXT, &advance),
                backend.bind_host(WIDE, |_, high, low| {
                    (i64::from(high) << 32) | i64::from(low)
                }),
            ];
            let instance = backend.instantiate(&module, &imports).await.unwrap();
            backend.function(&instance, EXECUTE).unwrap()
        };

        // Module and instance handles have been dropped; imported resources and
        // callable functions remain alive in the backend.
        backend.write_memory(&memory, 0, &7_u64.to_le_bytes());
        for expected in [0x1_0000_000c, 0x2_0000_0011] {
            assert_eq!(backend.call(&entry).unwrap(), expected);
            let mut bytes = [0; 8];
            backend.read_memory(&memory, 0, &mut bytes);
            assert_eq!(i64::from_le_bytes(bytes), expected);
        }
    });
}
