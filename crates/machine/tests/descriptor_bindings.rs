use relocade_compiler::{abi::Function, Program, I64};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler};
use relocade_x86::{abi::QUERY_SEGMENT_DESCRIPTOR, SegmentDescriptorInfo};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_descriptor_binding() {
    const CHECK: Function<fn(SegmentDescriptorInfo) -> u64> = Function::new("test", "check");
    const RUN: Function<fn() -> I64> = Function::new("test", "run");
    const QUERY: Function<fn(u16) -> SegmentDescriptorInfo> = Function::new("test", "query");
    let mut program = Program::new();
    let query = QUERY_SEGMENT_DESCRIPTOR.import(&mut program);
    let check = CHECK
        .define(&mut program, |body, (info,)| {
            let flags = info
                .lar_valid
                .unsigned()
                .extend::<I64>()
                .or(info.lsl_valid.unsigned().extend::<I64>().shl(1))
                .or(info.readable.unsigned().extend::<I64>().shl(2))
                .or(info.writable.unsigned().extend::<I64>().shl(3));
            body.return_(
                flags
                    .or(info.access_rights.unsigned().extend::<I64>().shl(8))
                    .or(info.limit.unsigned().extend::<I64>().shl(32)),
            )
        })
        .unwrap();
    RUN.define(&mut program, |mut body, ()| {
        let descriptor = query.call(&mut body, 0xffff)?;
        check.tail_call(body, descriptor)
    })
    .unwrap();
    QUERY
        .define(&mut program, |mut body, (selector,)| {
            let descriptor = query.call(&mut body, selector)?;
            body.return_(descriptor)
        })
        .unwrap();
    let mut backend = Backend::new().unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let imports = [backend.bind_host(
        QUERY_SEGMENT_DESCRIPTOR,
        Box::new(|_, selector| {
            assert_eq!(selector, 0xffff_u16);
            SegmentDescriptorInfo {
                lar_valid: true,
                lsl_valid: false,
                readable: true,
                writable: false,
                access_rights: 0x00cf_f300,
                limit: 0x8765_4321,
            }
        }),
    )];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    assert_eq!(backend.call(&run, ()), 0x8765_4321_cff3_0005_u64 as i64);
    let query = backend.function(&instance, QUERY).unwrap();
    assert_eq!(
        backend.call(&query, (0xffff,)),
        SegmentDescriptorInfo {
            lar_valid: true,
            lsl_valid: false,
            readable: true,
            writable: false,
            access_rights: 0x00cf_f300,
            limit: 0x8765_4321,
        }
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn descriptor_callbacks_preserve_named_fields() {
    pollster::block_on(check_descriptor_binding());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn descriptor_callbacks_preserve_named_fields() {
    check_descriptor_binding().await;
}
