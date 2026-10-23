use relocade_compiler::{
    abi::{Function, Memory, Table},
    Program, F64, I1, I16, I32, I64, I8,
};
use relocade_wasm_runtime::{WasmBackend, WasmCompiler, WasmContext, WasmMemoryAccess};

const MEMORY: Memory = Memory::new("example", "memory", 1);
const FUNCTIONS: Table<fn() -> I64> = Table::new("example", "functions", 1);
const CALLS: Function<fn() -> I32> = Function::new("example", "calls");
const SET_CALLS: Function<fn(I32)> = Function::new("example", "setCalls");
const READ: Function<fn(I32, I32) -> I64> = Function::new("example", "read");
const WRITE: Function<fn(I32, I32, I64)> = Function::new("example", "write");
const NEXT: Function<fn() -> I64> = Function::new("example", "next");
const RUN: Function<fn() -> I64> = Function::new("example", "run");

use relocade_wasm_runtime::Backend;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct HostState {
    calls: i32,
}

async fn check_stateful_callbacks() {
    let state = HostState { calls: 0 };
    let mut backend = Backend::with_state(state).unwrap();
    let memory = backend.create_memory(MEMORY).unwrap();
    let table = backend.create_table(1).unwrap();
    let entry = {
        // Snapshot blocks also use names constructed at runtime.
        let export_name = format!("advance_{}", 7);
        let advance_export: Function<fn() -> I64> = Function::new("example", &export_name);
        let mut producer = Program::new();
        let shared = producer.import_memory(MEMORY);
        let calls = producer.import_function(CALLS);
        let set_calls = producer.import_function(SET_CALLS);
        let read = producer.import_function(READ);
        let write = producer.import_function(WRITE);
        let advance = producer
            .function::<fn() -> I64>(|mut body, ()| {
                let count = body.call::<I32>(calls, &[])?.add(1);
                body.call::<()>(set_calls, &[count.clone().into()])?;
                let value = body.call::<I64>(read, &[0.into(), 8.into()])?;
                let next = value.add(count.signed().extend::<I64>());
                body.call::<()>(write, &[0.into(), 8.into(), next.into()])?;
                // The guest observes the callback's write through its own memory import.
                let current = body.load::<I64>(shared, 0)?;
                body.return_(current)
            })
            .unwrap();
        advance_export.export(&mut producer, advance).unwrap();
        let module = backend
            .compiler()
            .compile(&producer.compile().unwrap())
            .await
            .unwrap();
        let read_memory = memory.clone();
        let write_memory = memory.clone();
        let imports = [
            backend.bind(MEMORY, &memory),
            backend.bind_host(CALLS, Box::new(|caller| caller.state().calls)),
            backend.bind_host(
                SET_CALLS,
                Box::new(|mut caller, value| caller.state_mut().calls = value),
            ),
            backend.bind_host(
                READ,
                Box::new(move |caller, offset, _| {
                    let mut bytes = [0; 8];
                    caller.read_memory(&read_memory, offset as u32, &mut bytes);
                    i64::from_le_bytes(bytes)
                }),
            ),
            backend.bind_host(
                WRITE,
                Box::new(move |mut caller, offset, _, value| {
                    caller.write_memory(&write_memory, offset as u32, &value.to_le_bytes());
                }),
            ),
        ];
        let instance = backend.instantiate(&module, &imports).await.unwrap();
        let advance = backend.function(&instance, advance_export).unwrap();
        backend.set_table_function(&table, 0, &advance).unwrap();

        // The consumer shares memory and imports the producer's export under a new name.
        let mut consumer = Program::new();
        let shared = consumer.import_memory(MEMORY);
        let direct = consumer.import_function(NEXT);
        let functions = consumer.import_table(FUNCTIONS);
        let run = consumer
            .function::<fn() -> I64>(|mut body, ()| {
                let current = body.load::<I64>(shared, 0)?;
                body.store(shared, 0, current.add(4))?;
                body.call::<I64>(direct, &[])?;
                body.tail_call_indirect(functions, 0, &[])
            })
            .unwrap();
        RUN.export(&mut consumer, run).unwrap();
        let module = backend
            .compiler()
            .compile(&consumer.compile().unwrap())
            .await
            .unwrap();
        let instance = backend
            .instantiate(
                &module,
                &[
                    backend.bind(MEMORY, &memory),
                    backend.bind(NEXT, &advance),
                    backend.bind(FUNCTIONS, &table),
                ],
            )
            .await
            .unwrap();
        backend.function(&instance, RUN).unwrap()
    };

    // Functions retain their linked resources after module/instance handles drop.
    // Both signs exceed the exact integer range of JavaScript Number.
    let mut memory_size = 65536;
    for initial in [0x20_0000_0000_0001_i64, -0x20_0000_0000_0021_i64] {
        backend.state_mut().calls = 0;
        backend.write_memory(&memory, 0, &initial.to_le_bytes());
        // Each invocation adds four in the consumer, then the next two callback counts.
        for (calls, increment) in [(2, 7), (4, 18)] {
            assert_eq!(backend.call(&entry, ()), initial + increment);
            assert_eq!(backend.state().calls, calls);
            let mut bytes = [0; 8];
            backend.read_memory(&memory, 0, &mut bytes);
            assert_eq!(i64::from_le_bytes(bytes), initial + increment);
            memory_size += 65536;
            backend.ensure_memory_size(&memory, memory_size).unwrap();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stateful_callbacks_share_live_memory() {
    pollster::block_on(check_stateful_callbacks());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stateful_callbacks_share_live_memory() {
    check_stateful_callbacks().await;
}

async fn check_scalar_callbacks() {
    type NarrowSignature = fn(I1, I8, I16, I32) -> I16;
    const NARROW: Function<NarrowSignature> = Function::new("scalar", "narrow");
    const WIDE: Function<fn(I64) -> I64> = Function::new("scalar", "wide");
    const FLOAT: Function<fn(F64, I16) -> F64> = Function::new("scalar", "float");

    let mut backend = Backend::new().unwrap();
    let entry = {
        let mut program = Program::new();
        let narrow = program.import_function(NARROW);
        let wide = program.import_function(WIDE);
        let float = program.import_function(FLOAT);
        let run = program
            .function::<fn() -> I64>(|mut body, ()| {
                let narrow =
                    body.call::<I16>(narrow, &[1.into(), 255.into(), 65535.into(), (-1).into()])?;
                let float = body.call::<F64>(float, &[1.25.into(), narrow.into()])?;
                let wide = body.call::<I64>(wide, &[((-0x20_0000_0000_0001_i64) as u64).into()])?;
                body.return_(wide.xor(float.to_bits()))
            })
            .unwrap();
        RUN.export(&mut program, run).unwrap();
        let module = backend
            .compiler()
            .compile(&program.compile().unwrap())
            .await
            .unwrap();
        let imports = [
            backend.bind_host(
                NARROW,
                Box::new(|_, bit, byte, word, signed| {
                    assert_eq!((bit, byte, word, signed), (1, 255, 65535, -1));
                    0xffff
                }),
            ),
            backend.bind_host(
                WIDE,
                Box::new(|_, value| {
                    assert_eq!(value, -0x20_0000_0000_0001);
                    value - 2
                }),
            ),
            backend.bind_host(
                FLOAT,
                Box::new(|_, value, word| {
                    assert_eq!(word, 65535);
                    value + 0.5
                }),
            ),
        ];
        let instance = backend.instantiate(&module, &imports).await.unwrap();
        backend.function(&instance, RUN).unwrap()
    };
    assert_eq!(
        backend.call(&entry, ()),
        -0x20_0000_0000_0003_i64 ^ 1.75_f64.to_bits() as i64
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_preserve_scalar_carriers() {
    pollster::block_on(check_scalar_callbacks());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_preserve_scalar_carriers() {
    check_scalar_callbacks().await;
}

async fn check_tuple_callbacks() {
    type Mixed = (I1, I8, I16, I32, I64, F64);
    const MIXED: Function<fn() -> Mixed> = Function::new("tuple", "mixed");
    const SINGLE: Function<fn() -> (I64,)> = Function::new("tuple", "single");
    let mut backend = Backend::new().unwrap();
    let memory = backend.create_memory(MEMORY).unwrap();
    let mut program = Program::new();
    let output = program.import_memory(MEMORY);
    let mixed = program.import_function(MIXED);
    let single = program.import_function(SINGLE);
    let run = program
        .function::<fn() -> I64>(|mut body, ()| {
            let (bit, byte, word, integer, wide, float) = body.call::<Mixed>(mixed, &[])?;
            body.store(output, 0, bit.unsigned().extend::<I32>())?;
            body.store(output, 4, byte.unsigned().extend::<I32>())?;
            body.store(output, 8, word.unsigned().extend::<I32>())?;
            body.store(output, 12, integer)?;
            body.store(output, 16, wide)?;
            body.store(output, 24, float)?;
            let (result,) = body.call::<(I64,)>(single, &[])?;
            body.return_(result)
        })
        .unwrap();
    RUN.export(&mut program, run).unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let imports = [
        backend.bind(MEMORY, &memory),
        backend.bind_host(
            MIXED,
            Box::new(|_| (1, 255, 65535, -1, -0x20_0000_0000_0001, 1.75)),
        ),
        backend.bind_host(SINGLE, Box::new(|_| (0x20_0000_0000_0003,))),
    ];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let entry = backend.function(&instance, RUN).unwrap();
    assert_eq!(backend.call(&entry, ()), 0x20_0000_0000_0003);
    let mut bytes = [0; 32];
    backend.read_memory(&memory, 0, &mut bytes);
    assert_eq!(
        &bytes[..16],
        &[1, 0, 0, 0, 255, 0, 0, 0, 255, 255, 0, 0, 255, 255, 255, 255]
    );
    assert_eq!(&bytes[16..24], &(-0x20_0000_0000_0001_i64).to_le_bytes());
    assert_eq!(&bytes[24..], &1.75_f64.to_le_bytes());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_preserve_flat_tuple_carriers() {
    pollster::block_on(check_tuple_callbacks());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_preserve_flat_tuple_carriers() {
    check_tuple_callbacks().await;
}
