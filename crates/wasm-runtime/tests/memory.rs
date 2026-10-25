use relocade_compiler::{
    abi::{Function, Memory},
    Program, I32, I64,
};
use relocade_wasm_runtime::{
    MemoryAccess, WasmBackend, WasmCompiler, WasmMemoryAccess, WasmSharedMemory,
};

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
                context.memory(&callback_memory).read(0, &mut bytes);
                assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12]);
                context
                    .memory(&callback_memory)
                    .write(4, &[0xab, 0xcd, 0xef, 0]);
                0
            }),
        ),
    ];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    assert_eq!(backend.call(&run, ()), 0x00efcdab12345678);

    let mut retained = reader.view();
    backend.ensure_memory_size(&memory, 2 * 65536).unwrap();
    retained.write(65536, &[0x55, 0x66]);
    let mut grown = [0; 2];
    retained.read(65536, &mut grown);
    assert_eq!(grown, [0x55, 0x66]);
    retained.store(65537, 0xfedc_ba98_7654_3210_u64);
    assert_eq!(reader.view().load::<u64>(65537), 0xfedc_ba98_7654_3210);
    assert!(backend.ensure_memory_size(&memory, 4 * 65536).is_err());
    // Stopped access and independent readers observe the same live allocation.
    {
        backend.memory(&memory).write(65534, &[1, 2, 3, 4]);
        let mut bytes = [0; 4];
        reader.view().read(65534, &mut bytes);
        assert_eq!(bytes, [1, 2, 3, 4]);
        for _ in 0..2 {
            backend.memory(&memory).write(0, &[0; 8]);
            assert_eq!(run.call(&mut backend, ()), 0x00efcdab12345678);
            backend.memory(&memory).read(4, &mut bytes);
            assert_eq!(bytes, [0xab, 0xcd, 0xef, 0]);
        }
    }
    drop(backend);
    // The reader owns the allocation, including its newly grown portion.
    let mut bytes = [0; 8];
    reader.clone().view().read(0, &mut bytes);
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn shared_custom_storage_keeps_live_access_across_growth() {
    use relocade_wasm_runtime::SharedMemoryStorage;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Shared(Arc<Mutex<Vec<u8>>>);
    impl SharedMemoryStorage for Shared {
        fn read(&self, offset: u32, bytes: &mut [u8]) {
            bytes.copy_from_slice(&self.0.lock().unwrap()[offset as usize..][..bytes.len()]);
        }
        fn write(&self, offset: u32, bytes: &[u8]) {
            self.0.lock().unwrap()[offset as usize..][..bytes.len()].copy_from_slice(bytes);
        }
    }
    impl WasmSharedMemory for Shared {
        fn byte_len(&self) -> u64 {
            self.0.lock().unwrap().len() as u64
        }
    }

    let shared = Shared(Arc::new(Mutex::new(vec![1, 2])));
    let other = shared.clone();
    let mut access = shared.view();
    other.0.lock().unwrap().extend_from_slice(&[3, 4]);
    let mut bytes = [0; 4];
    access.read(0, &mut bytes);
    assert_eq!(bytes, [1, 2, 3, 4]);
    access.reborrow().write(2, &[5, 6]);
    other.view().read(0, &mut bytes);
    assert_eq!(bytes, [1, 2, 5, 6]);
    assert_eq!(access.load::<u32>(0), 0x0605_0201);
    access.store(1, 0xfe_dc_u16);
    other.view().read(0, &mut bytes);
    assert_eq!(bytes, [1, 0xdc, 0xfe, 6]);
}

fn check_scalars(access: &mut MemoryAccess<'_>, offset: u32) {
    access.write(offset, &[0xa5; 24]);
    access.store(offset + 1, 0x81_u8);
    access.store(offset + 3, 0xfedc_u16);
    access.store(offset + 5, 0xfedc_ba98_u32);
    access.store(offset + 9, 0xfedc_ba98_7654_3210_u64);
    let mut bytes = [0; 24];
    access.read(offset, &mut bytes);
    assert_eq!(
        bytes,
        [
            0xa5, 0x81, 0xa5, 0xdc, 0xfe, 0x98, 0xba, 0xdc, 0xfe, 0x10, 0x32, 0x54, 0x76, 0x98,
            0xba, 0xdc, 0xfe, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5,
        ]
    );
    access.write(
        offset,
        &[
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x0f, 0xed, 0xcb, 0xa9, 0x87, 0x65,
            0x43, 0x21,
        ],
    );
    assert_eq!(access.load::<u8>(offset + 1), 0x34);
    assert_eq!(access.load::<u16>(offset + 1), 0x5634);
    assert_eq!(access.load::<u32>(offset + 1), 0x9a78_5634);
    assert_eq!(access.load::<u64>(offset + 1), 0x0ff0_debc_9a78_5634);
    assert_eq!(access.load::<u64>(offset), 0xf0de_bc9a_7856_3412);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn scalar_and_bulk_transfers_follow_memory_growth() {
    for shared in [false, true] {
        let mut backend = Backend::new().unwrap();
        let memory = backend.create_memory(Memory { shared, ..SHARED }).unwrap();
        check_scalars(&mut backend.memory(&memory), 0);
        backend.ensure_memory_size(&memory, 2 * 65536).unwrap();
        let mut access = backend.memory(&memory);
        check_scalars(&mut access, 65529);
        check_scalars(&mut access.reborrow(), 65537);
        let input: Vec<u8> = (0..4097).map(|i| (i * 13) as u8).collect();
        let mut output = vec![0; input.len()];
        access.write(65520, &input);
        access.read(65520, &mut output);
        assert_eq!(output, input);
        access.write(2 * 65536, &[]);
        access.read(2 * 65536, &mut []);
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn transfers_follow_rust_memory_growth() {
    use wasm_bindgen::JsCast;
    let mut backend = Backend::new().unwrap();
    let memory = backend
        .create_memory(Memory {
            shared: false,
            ..SHARED
        })
        .unwrap();
    let mut access = backend.memory(&memory);
    check_scalars(&mut access, 0);
    wasm_bindgen::memory()
        .unchecked_into::<js_sys::WebAssembly::Memory>()
        .grow(1);
    check_scalars(&mut access, 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn custom_storage_uses_little_endian_byte_transfers() {
    use relocade_wasm_runtime::MemoryStorage;
    struct Bytes([u8; 24]);
    impl MemoryStorage for Bytes {
        fn read(&self, offset: u32, bytes: &mut [u8]) {
            bytes.copy_from_slice(&self.0[offset as usize..][..bytes.len()]);
        }
        fn write(&mut self, offset: u32, bytes: &[u8]) {
            self.0[offset as usize..][..bytes.len()].copy_from_slice(bytes);
        }
    }
    check_scalars(&mut MemoryAccess::from_storage(&mut Bytes([0; 24])), 0);
}
