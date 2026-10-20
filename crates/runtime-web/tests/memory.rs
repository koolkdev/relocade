use relocade_compiler::{
    abi::{Function, Memory},
    Program, I32, I64,
};
use relocade_runtime::wasm::{WasmBackend, WasmMemoryAccess, WasmSharedMemory};

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const SHARED: Memory = Memory {
    shared: true,
    maximum: Some(3),
    ..Memory::new("example", "memory", 1)
};
const WRITE: Function<fn() -> I32> = Function::new("example", "write");
const RUN: Function<fn() -> I64> = Function::new("example", "run");

fn backend() -> Backend {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Backend::new().unwrap()
    }
    #[cfg(target_arch = "wasm32")]
    {
        Backend::new()
    }
}

async fn check_shared_memory() {
    let mut backend = backend();
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
    let run = program
        .function(RUN.signature(), |mut body| {
            // Ordinary Wasm stores remain valid in a shared memory.
            body.store::<I32>(guest, 0, 0x12345678_u32)?;
            body.call::<I32>(write, &[])?;
            let value = body.load::<I64>(guest, 0)?;
            body.return_(value)
        })
        .unwrap();
    RUN.export(&mut program, run).unwrap();
    let module = backend.compile(&program.compile().unwrap()).await.unwrap();
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
    assert_eq!(backend.call(&run).unwrap(), 0x00efcdab12345678);

    backend.ensure_memory_size(&memory, 2 * 65536).unwrap();
    assert!(backend.ensure_memory_size(&memory, 4 * 65536).is_err());
    // A pending host service can hold this context while another owner reads.
    {
        let mut context = backend.context();
        context.write_memory(&memory, 65534, &[1, 2, 3, 4]);
        let mut bytes = [0; 4];
        reader.read(65534, &mut bytes);
        assert_eq!(bytes, [1, 2, 3, 4]);
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_reader_can_move_to_another_thread() {
    let mut backend = backend();
    let memory = backend.create_memory(SHARED).unwrap();
    let reader = backend.shared_memory(&memory).unwrap();
    let (send, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        receive.recv().unwrap();
        let mut bytes = [0; 4];
        reader.read(65536, &mut bytes);
        bytes
    });
    backend.ensure_memory_size(&memory, 2 * 65536).unwrap();
    backend.write_memory(&memory, 65536, &[1, 2, 3, 4]);
    send.send(()).unwrap();
    assert_eq!(thread.join().unwrap(), [1, 2, 3, 4]);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shared_memory_can_be_sent_to_a_worker() {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(module = "/tests/shared_memory.js")]
    extern "C" {
        #[wasm_bindgen(catch)]
        async fn write_in_worker(memory: &js_sys::WebAssembly::Memory) -> Result<JsValue, JsValue>;
    }
    let mut backend = backend();
    let memory = backend.create_memory(SHARED).unwrap();
    let reader = backend.shared_memory(&memory).unwrap();
    backend.write_memory(&memory, 0, &[42]);
    assert_eq!(
        write_in_worker(reader.memory()).await.unwrap().as_f64(),
        Some(42.0)
    );
    let imported =
        relocade_runtime_web::SharedMemory::from_memory(reader.memory().clone()).unwrap();
    let mut bytes = [0; 4];
    imported.read(65536, &mut bytes);
    assert_eq!(bytes, [1, 2, 3, 4]);
}
