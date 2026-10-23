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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn memory_transfers_preserve_ranges_after_growth() {
    let mut backend = Backend::new().unwrap();
    for shared in [false, true] {
        let memory = backend.create_memory(Memory { shared, ..SHARED }).unwrap();
        // Exercise the established transfer functions before growing either memory.
        backend.write_memory(&memory, 31, &[0x12, 0x34, 0x56]);
        let mut original = [0; 3];
        backend.read_memory(&memory, 31, &mut original);
        assert_eq!(original, [0x12, 0x34, 0x56]);
        backend.ensure_memory_size(&memory, 2 * 65536).unwrap();
        #[cfg(target_arch = "wasm32")]
        assert_ne!(std::arch::wasm32::memory_grow::<0>(1), usize::MAX);

        for length in [0, 1, 7, 8, 31, 129, 4097] {
            let source: Vec<_> = (0..length).map(|i| (i * 17 + 3) as u8).collect();
            let offset = 65533;
            backend.write_memory(&memory, offset - 1, &vec![0xa5; length + 2]);
            backend.write_memory(&memory, offset, &source);
            let mut destination = vec![0x5a; length + 4];
            backend.read_memory(&memory, offset - 1, &mut destination[1..length + 3]);
            assert_eq!(destination[0], 0x5a);
            assert_eq!(destination[1], 0xa5);
            assert_eq!(&destination[2..length + 2], source);
            assert_eq!(destination[length + 2], 0xa5);
            assert_eq!(destination[length + 3], 0x5a);
        }
        backend.write_memory(&memory, 2 * 65536, &[]);
        backend.read_memory(&memory, 2 * 65536, &mut []);
    }
}

fn check_scalars<A: WasmMemoryAccess>(access: &mut A, memory: &A::Memory, offset: u32) {
    macro_rules! check {
        ($type:ty, $value:expr, $bytes:expr) => {{
            let expected = $bytes;
            access.write_memory(memory, offset, &expected);
            assert_eq!(access.load_memory::<$type>(memory, offset), $value);
            access.write_memory(memory, offset - 1, &vec![0xa5; expected.len() + 2]);
            access.store_memory(memory, offset, $value);
            let mut actual = vec![0; expected.len() + 2];
            access.read_memory(memory, offset - 1, &mut actual);
            assert_eq!(actual[0], 0xa5);
            assert_eq!(&actual[1..=expected.len()], expected);
            assert_eq!(actual[expected.len() + 1], 0xa5);
        }};
    }
    check!(u8, 0xd2_u8, [0xd2]);
    check!(u16, 0xb7d2_u16, [0xd2, 0xb7]);
    check!(u32, 0x89ab_cdef_u32, [0xef, 0xcd, 0xab, 0x89]);
    check!(
        u64,
        0xfedc_ba98_7654_3210_u64,
        [0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn scalar_transfers_preserve_widths_after_growth() {
    let mut backend = Backend::new().unwrap();
    for shared in [false, true] {
        let memory = backend.create_memory(Memory { shared, ..SHARED }).unwrap();
        check_scalars(&mut backend, &memory, 17);
        backend.ensure_memory_size(&memory, 2 * 65536).unwrap();
        #[cfg(target_arch = "wasm32")]
        assert_ne!(std::arch::wasm32::memory_grow::<0>(1), usize::MAX);
        check_scalars(&mut backend, &memory, 65533);
        check_scalars(&mut backend.context(), &memory, 65539);
        // A wider load must not read beyond a valid scalar's allocated range.
        let end = 2 * 65536;
        backend.write_memory(
            &memory,
            end - 8,
            &[0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe],
        );
        assert_eq!(backend.load_memory::<u8>(&memory, end - 1), 0xfe);
        assert_eq!(backend.load_memory::<u16>(&memory, end - 2), 0xfedc);
        assert_eq!(backend.load_memory::<u32>(&memory, end - 4), 0xfedc_ba98);
        assert_eq!(
            backend.load_memory::<u64>(&memory, end - 8),
            0xfedc_ba98_7654_3210
        );
    }
}

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
    let run = program
        .function::<fn() -> I64>(|mut body, ()| {
            // Ordinary Wasm stores remain valid in a shared memory.
            body.store::<I32>(guest, 0, 0x12345678_u32)?;
            body.call::<I32>(write, &[])?;
            let value = body.load::<I64>(guest, 0)?;
            body.return_(value)
        })
        .unwrap();
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
                assert_eq!(context.load_memory::<u32>(&callback_memory, 0), 0x1234_5678);
                context.store_memory::<u32>(&callback_memory, 4, 0x00ef_cdab);
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
        let mut context = backend.context();
        context.write_memory(&memory, 65534, &[1, 2, 3, 4]);
        let mut bytes = [0; 4];
        reader.read(65534, &mut bytes);
        assert_eq!(bytes, [1, 2, 3, 4]);
        reader.store::<u16>(65535, 0xface);
        assert_eq!(context.load_memory::<u32>(&memory, 65534), 0x04fa_ce01);
    }
    drop(backend);
    // The reader owns the allocation, including its newly grown portion.
    let mut bytes = [0; 8];
    reader.clone().read(0, &mut bytes);
    assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12, 0xab, 0xcd, 0xef, 0]);
    assert_eq!(reader.load::<u64>(0), 0x00ef_cdab_1234_5678);
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
