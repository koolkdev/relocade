use relocade_compiler::{
    abi::{Function, Memory},
    Program, I32, I64,
};
use relocade_wasm_runtime::{WasmBackend, WasmCompiler, WasmMemoryAccess, WasmSharedMemory};

use relocade_wasm_runtime::Backend;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const SHARED: Memory = Memory {
    shared: true,
    maximum: Some(3),
    ..Memory::new("example", "memory", 1)
};
const WRITE: Function<fn() -> I32> = Function::new("example", "write");
const RUN: Function<fn() -> I64> = Function::new("example", "run");

async fn check_shared_memory() {
    let mut backend = Backend::new().unwrap();
    let private = backend
        .create_memory(Memory {
            shared: false,
            ..SHARED
        })
        .unwrap();
    assert!(backend.shared_memory(&private).is_none());
    let memory = backend.create_memory(SHARED).unwrap();
    let reader = backend.shared_memory(&memory).unwrap();

    let mut program = Program::new();
    let guest = program.import_memory(SHARED);
    let write = program.import_function(WRITE);
    let run = program.function::<fn() -> I64>(move |mut body, ()| {
        // Ordinary Wasm stores remain valid in a shared memory.
        body.store::<I32>(guest, 0, 0x12345678_u32)?;
        body.call::<I32>(write, &[])?;
        let value = body.load::<I64>(guest, 0)?;
        body.return_(value)
    });
    RUN.export(&mut program, run).unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let callback_memory = memory.clone();
    let imports = [
        backend.bind(SHARED, &memory),
        backend.bind_host(
            WRITE,
            Box::new(move |mut context| {
                let mut bytes = [0; 4];
                context.read_memory(&callback_memory, 0, &mut bytes);
                assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12]);
                context.write_memory(&callback_memory, 4, &[0xab, 0xcd, 0xef, 0]);
                0
            }),
        ),
    ];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    assert_eq!(backend.call(&run, ()), 0x00efcdab12345678);

    backend.ensure_memory_size(&memory, 2 * 65536).unwrap();
    assert!(backend.ensure_memory_size(&memory, 4 * 65536).is_err());
    // A pending host service can hold this context while another owner reads.
    {
        let mut context = backend.execution();
        context.write_memory(&memory, 65534, &[1, 2, 3, 4]);
        let mut bytes = [0; 4];
        reader.read(65534, &mut bytes);
        assert_eq!(bytes, [1, 2, 3, 4]);
        for _ in 0..2 {
            context.write_memory(&memory, 0, &[0; 8]);
            assert_eq!(run.call(&mut context, ()), 0x00efcdab12345678);
            context.read_memory(&memory, 4, &mut bytes);
            assert_eq!(bytes, [0xab, 0xcd, 0xef, 0]);
        }
    }
    drop(backend);
    // The reader owns the allocation, including its newly grown portion.
    let mut bytes = [0; 8];
    reader.clone().read(0, &mut bytes);
    assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12, 0xab, 0xcd, 0xef, 0]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_memory_outlives_the_backend_and_tracks_growth() {
    pollster::block_on(check_shared_memory());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shared_memory_outlives_the_backend_and_tracks_growth() {
    check_shared_memory().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn deserialization_rejects_private_memory() {
    let descriptor = js_sys::Object::new();
    js_sys::Reflect::set(&descriptor, &"initial".into(), &1.into()).unwrap();
    let private = js_sys::WebAssembly::Memory::new(&descriptor).unwrap();
    assert!(
        serde_wasm_bindgen::from_value::<relocade_wasm_runtime::SharedMemory>(private.into())
            .is_err()
    );
}
