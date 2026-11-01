use relocade_compiler::{
    abi::{Function, Memory},
    Program, I32, I64,
};
use relocade_wasm_runtime::{
    MemoryAccess, SharedMemory, WasmBackend, WasmCompiler, WasmMemoryAccess, WasmSharedMemory,
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
    assert!(SharedMemory::new(2, 1).is_err());
    assert!(SharedMemory::new(0, 65537).is_err());
    let reader = SharedMemory::new(SHARED.minimum, SHARED.maximum.unwrap()).unwrap();
    assert_eq!(reader.byte_len(), 65536);
    assert_eq!(reader.view().load::<u64>(0), 0);
    let mut backend = Backend::new().unwrap();
    let private = backend
        .create_memory(Memory {
            shared: false,
            ..SHARED
        })
        .unwrap();
    assert!(backend.shared_memory(&private).is_none());
    let memory = backend.import_shared_memory(&reader);
    backend.shared_memory(&memory).unwrap().write(0, &[0x42]);
    assert_eq!(reader.view().load::<u8>(0), 0x42);

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
    retained.reborrow().write(65536, &[0x55, 0x66]);
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
fn shared_scalars_track_growth_and_owner_drop() {
    for minimum in [0, 1] {
        let memory = SharedMemory::new(minimum, 2).unwrap();
        let retained = memory.clone();
        let mut backend = Backend::new().unwrap();
        let imported = backend.import_shared_memory(&memory);
        memory.write(minimum * 65536, &[]);
        backend.ensure_memory_size(&imported, 2 * 65536).unwrap();
        // One value straddles the original allocation's page boundary. Existing
        // handles and views must observe the newly grown bytes too.
        retained.store(65533, 0xfedc_ba98_7654_3210_u64);
        assert_eq!(memory.view().load::<u64>(65533), 0xfedc_ba98_7654_3210);
        memory.write(0, &[0x81]);
        assert_eq!(retained.load::<u8>(0), 0x81);
        drop(backend);
        drop(imported);
        drop(memory);
        let clone = retained.clone();
        clone.store(1, 0xfedc_ba98_7654_3210_u64);
        clone.store(3, 0x1234_u16);
        clone.store(20, 0xaabb_ccdd_u32);
        let mut bytes = [0; 4];
        retained.read(20, &mut bytes);
        assert_eq!(bytes, [0xdd, 0xcc, 0xbb, 0xaa]);
        assert_eq!(retained.load::<u16>(3), 0x1234);
        assert_eq!(retained.load::<u32>(5), 0xfedc_ba98);
        assert_eq!(retained.load::<u64>(1), 0xfedc_ba98_1234_3210);
        assert_eq!(retained.load::<u64>(65533), 0xfedc_ba98_7654_3210);
        retained.store(2 * 65536 - 1, 0x42_u8);
        assert_eq!(clone.load::<u8>(2 * 65536 - 1), 0x42);
        retained.write(2 * 65536, &[]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_memory_can_be_accessed_and_grown_on_another_thread() {
    let memory = std::sync::Arc::new(SharedMemory::new(1, 2).unwrap());
    memory.store(1, 0x1234_u16);
    let writer = memory.clone();
    std::thread::spawn(move || {
        assert_eq!(writer.load::<u16>(1), 0x1234);
        let mut backend = Backend::new().unwrap();
        let imported = backend.import_shared_memory(&writer);
        backend.ensure_memory_size(&imported, 2 * 65536).unwrap();
        writer.store(65535, 0xfedc_ba98_u32);
        writer.store(1, 0x5678_u16);
    })
    .join()
    .unwrap();
    assert_eq!(memory.load::<u32>(65535), 0xfedc_ba98);
    assert_eq!(memory.load::<u16>(1), 0x5678);
}
