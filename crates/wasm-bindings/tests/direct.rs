#![cfg(target_arch = "wasm32")]

use js_sys::{Function as JsFunction, Object, Reflect, Uint8Array, WebAssembly};
use relocade_wasm_bindings::{callback, Function};
use std::{cell::Cell, rc::Rc};
use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, Function as WasmFunction, FunctionSection,
    ImportSection, Instruction, Module, TypeSection, ValType,
};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[path = "direct/lifetime.rs"]
mod lifetime;

// A raw Wasm signature keeps the test independent of Rust's binding metadata.
fn echo(types: &[ValType], callback: Option<&JsFunction>) -> WebAssembly::Instance {
    let mut module = Module::new();
    let mut signatures = TypeSection::new();
    signatures
        .ty()
        .function(types.iter().copied(), types.iter().copied());
    module.section(&signatures);
    let imports = Object::new();
    if let Some(callback) = callback {
        let mut functions = ImportSection::new();
        functions.import("host", "call", EntityType::Function(0));
        module.section(&functions);
        let host = Object::new();
        Reflect::set(&host, &"call".into(), callback).unwrap();
        Reflect::set(&imports, &"host".into(), &host).unwrap();
    }
    let mut functions = FunctionSection::new();
    functions.function(0);
    module.section(&functions);
    let mut exports = ExportSection::new();
    exports.export("echo", ExportKind::Func, u32::from(callback.is_some()));
    module.section(&exports);
    let mut body = WasmFunction::new([]);
    for index in 0..types.len() as u32 {
        body.instruction(&Instruction::LocalGet(index));
    }
    if callback.is_some() {
        body.instruction(&Instruction::Call(0));
    }
    body.instruction(&Instruction::End);
    let mut code = CodeSection::new();
    code.function(&body);
    module.section(&code);
    let module = WebAssembly::Module::new(&Uint8Array::from(module.finish().as_slice())).unwrap();
    WebAssembly::Instance::new(&module, &imports).unwrap()
}

fn forbid_js_calls(function: &JsFunction) {
    let fail = JsFunction::new_no_args("throw new Error('unexpected JavaScript call bridge')");
    Reflect::set(function, &"call".into(), &fail).unwrap();
    Reflect::set(function, &"apply".into(), &fail).unwrap();
}

type Scalars = (f32, i64, f32, i32, f64, i64, f32, i32, f64, i32, i64, f64);
const TYPES: [ValType; 12] = [
    ValType::F32,
    ValType::I64,
    ValType::F32,
    ValType::I32,
    ValType::F64,
    ValType::I64,
    ValType::F32,
    ValType::I32,
    ValType::F64,
    ValType::I32,
    ValType::I64,
    ValType::F64,
];

fn values() -> Scalars {
    (
        f32::from_bits(0xffc1_2345),
        i64::MIN,
        -0.0,
        i32::MIN,
        f64::from_bits(0xfff8_1234_5678_9abc),
        -1,
        f32::from_bits(1),
        0x1234_5678,
        -0.0,
        -1,
        0x0020_0000_0000_0001,
        f64::from_bits(1),
    )
}

fn assert_values(value: Scalars) {
    assert_eq!(
        (value.1, value.3, value.5, value.7, value.9, value.10),
        (
            i64::MIN,
            i32::MIN,
            -1,
            0x1234_5678,
            -1,
            0x0020_0000_0000_0001
        ),
    );
    assert_eq!(
        [value.0.to_bits(), value.2.to_bits(), value.6.to_bits()],
        [0xffc1_2345, 0x8000_0000, 1],
    );
    assert_eq!(
        [value.4.to_bits(), value.8.to_bits(), value.11.to_bits()],
        [0xfff8_1234_5678_9abc, 0x8000_0000_0000_0000, 1],
    );
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn exports_preserve_scalar_bits_and_keep_the_original_multivalue_signature() {
    let function = Function::<Scalars, Scalars>::from_export(&echo(&TYPES, None), "echo").unwrap();
    forbid_js_calls(function.as_function());
    assert_values(function.call(values()));
    let imported = echo(&TYPES, Some(function.as_function()));
    drop(function);
    let rebound = Function::<Scalars, Scalars>::from_export(&imported, "echo").unwrap();
    assert_values(rebound.call(values()));
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn unit_and_scalar_functions_support_direct_calls_and_callbacks() {
    let calls = Rc::new(Cell::new(0));
    let observed = Rc::clone(&calls);
    let target = callback::<(), ()>(move |()| observed.set(observed.get() + 1)).unwrap();
    Function::<(), ()>::new(&target).unwrap().call(());
    assert_eq!(calls.get(), 1);

    let function =
        Function::<(f32,), (f32,)>::from_export(&echo(&[ValType::F32], None), "echo").unwrap();
    for bits in [0x8000_0000, 0xffc1_2345, 1] {
        assert_eq!(function.call((f32::from_bits(bits),)).0.to_bits(), bits);
    }
    let target = callback::<(i64,), (i64,)>(|value| value).unwrap();
    assert_eq!(
        Function::<(i64,), (i64,)>::new(&target)
            .unwrap()
            .call((-1,)),
        (-1,)
    );
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn aggregate_callbacks_can_reenter_the_same_function() {
    type Call = Function<(i32,), (f32, i64, f32)>;
    thread_local! {
        static CALL: std::cell::RefCell<Option<Call>> = const { std::cell::RefCell::new(None) };
    }
    let target = callback::<(i32,), (f32, i64, f32)>(|(depth,)| {
        if depth != 0 {
            let (first, integer, last) =
                CALL.with(|call| call.borrow().as_ref().unwrap().call((depth - 1,)));
            assert_eq!(first.to_bits(), 0x8000_0000 | (depth - 1) as u32);
            assert_eq!(integer, i64::MIN + i64::from(depth - 1));
            assert_eq!(last.to_bits(), 0xffc1_2345);
        }
        (
            f32::from_bits(0x8000_0000 | depth as u32),
            i64::MIN + i64::from(depth),
            f32::from_bits(0xffc1_2345),
        )
    })
    .unwrap();
    let outer = Call::new(&target).unwrap();
    CALL.with(|call| *call.borrow_mut() = Some(Call::new(&target).unwrap()));
    let (first, integer, last) = outer.call((3,));
    assert_eq!(first.to_bits(), 0x8000_0003);
    assert_eq!(integer, i64::MIN + 3);
    assert_eq!(last.to_bits(), 0xffc1_2345);
    CALL.with(|call| call.borrow_mut().take());
}

#[derive(relocade_wasm_bindings::WasmAbi, Debug, PartialEq)]
struct Request {
    code: u8,
    amount: u64,
    factor: f32,
    enabled: bool,
}

#[derive(relocade_wasm_bindings::WasmAbi, Debug, PartialEq)]
enum Reply {
    Missing,
    Found { request: Request, sample: f64 },
}

#[wasm_bindgen_test::wasm_bindgen_test]
fn derived_values_cross_imports_and_exports_without_a_compiler() {
    let target = callback::<Reply, Reply>(|reply| match reply {
        Reply::Missing => Reply::Missing,
        Reply::Found {
            mut request,
            sample,
        } => {
            assert_eq!(request.code, 255);
            assert_eq!(request.amount, u64::MAX);
            assert_eq!(request.factor.to_bits(), 0xffc1_2345);
            assert!(request.enabled);
            assert_eq!(sample.to_bits(), 0xfff8_1234_5678_9abc);
            request.code = 0;
            request.amount = 1 << 63;
            Reply::Found { request, sample }
        }
    })
    .unwrap();
    forbid_js_calls(&target);
    let instance = echo(
        &[
            ValType::I32,
            ValType::I32,
            ValType::I64,
            ValType::F32,
            ValType::I32,
            ValType::F64,
        ],
        Some(&target),
    );
    let function = Function::<Reply, Reply>::from_export(&instance, "echo").unwrap();
    assert_eq!(function.call(Reply::Missing), Reply::Missing);
    let Reply::Found { request, sample } = function.call(Reply::Found {
        request: Request {
            code: 255,
            amount: u64::MAX,
            factor: f32::from_bits(0xffc1_2345),
            enabled: true,
        },
        sample: f64::from_bits(0xfff8_1234_5678_9abc),
    }) else {
        panic!("found reply")
    };
    assert_eq!(
        (request.code, request.amount, request.enabled),
        (0, 1 << 63, true)
    );
    assert_eq!(request.factor.to_bits(), 0xffc1_2345);
    assert_eq!(sample.to_bits(), 0xfff8_1234_5678_9abc);
}
