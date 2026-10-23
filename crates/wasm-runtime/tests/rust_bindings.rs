use relocade_compiler::{
    abi::{AbiType, AbiValue, Carrier, Function},
    Program, I32, I64,
};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_rust_callbacks() {
    type Narrow = fn(bool, i8, i16, u32) -> (bool, i8, i16, u32);
    type Wide = fn(u64, f64) -> (u64, f64);
    const NARROW: Function<Narrow> = Function::new("rust", "narrow");
    const WIDE: Function<Wide> = Function::new("rust", "wide");
    const SINGLE: Function<fn() -> (i64,)> = Function::new("rust", "single");
    const OBSERVE: Function<fn(u32, u32, u32)> = Function::new("rust", "observe");
    const RUN: Function<fn() -> I64> = Function::new("rust", "run");
    let mut backend = Backend::with_state(0_u32).unwrap();
    let mut program = Program::new();
    let narrow = NARROW.import(&mut program);
    let wide = WIDE.import(&mut program);
    let single = SINGLE.import(&mut program);
    let observe = OBSERVE.import(&mut program);
    RUN.define(&mut program, move |mut body, ()| {
        let (bit, byte, word, integer) = narrow.call(&mut body, (true, 255, 65535, u32::MAX))?;
        observe.call(
            &mut body,
            (
                bit.unsigned().extend::<I32>(),
                byte.unsigned().extend::<I32>(),
                word.unsigned().extend::<I32>(),
            ),
        )?;
        let (wide, float) = wide.call(&mut body, (u64::MAX, 1.25))?;
        let (single,) = single.call(&mut body, ())?;
        body.return_(
            wide.xor(float.to_bits())
                .xor(single)
                .xor(integer.unsigned().extend::<I64>()),
        )
    })
    .unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let imports = [
        backend.bind_host(
            NARROW,
            Box::new(|_, bit, byte, word, integer| {
                assert_eq!((bit, byte, word, integer), (true, -1, -1, u32::MAX));
                (false, i8::MIN, i16::MIN, integer)
            }),
        ),
        backend.bind_host(
            WIDE,
            Box::new(|_, wide, float| {
                assert_eq!((wide, float), (u64::MAX, 1.25));
                (0x8000_0000_0000_0001, float + 0.5)
            }),
        ),
        backend.bind_host(SINGLE, Box::new(|_| (-0x20_0000_0000_0001,))),
        backend.bind_host(
            OBSERVE,
            Box::new(|mut context, bit, byte, word| {
                assert_eq!((bit, byte, word), (0, 128, 32768));
                *context.state_mut() += 1;
            }),
        ),
    ];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    assert_eq!(
        backend.call(&run, ()),
        (0x8000_0000_0000_0001_u64
            ^ 1.75_f64.to_bits()
            ^ (-0x20_0000_0000_0001_i64 as u64)
            ^ u64::from(u32::MAX)) as i64
    );
    assert_eq!(*backend.state(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_use_rust_values_and_preserve_logical_bits() {
    pollster::block_on(check_rust_callbacks());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_use_rust_values_and_preserve_logical_bits() {
    check_rust_callbacks().await;
}

static LIVE_ARGUMENTS: AtomicUsize = AtomicUsize::new(0);

struct OwnedArgument {
    ordinal: usize,
}

impl Drop for OwnedArgument {
    fn drop(&mut self) {
        LIVE_ARGUMENTS.fetch_sub(1, Ordering::SeqCst);
    }
}

impl AbiType for OwnedArgument {
    type Logical = I32;
}

impl AbiValue for OwnedArgument {
    type Value = Self;

    fn lower(_: Self) -> Carrier<Self> {
        u32::lower(0)
    }

    fn lift(value: Carrier<Self>) -> Option<Self> {
        if u32::lift(value)? != 0 {
            return None;
        }
        Some(Self {
            ordinal: LIVE_ARGUMENTS.fetch_add(1, Ordering::SeqCst),
        })
    }
}

async fn check_callback_arguments() {
    const ACCEPT: Function<fn(OwnedArgument, OwnedArgument) -> u32> =
        Function::new("rust", "accept");
    const RUN: Function<fn(u32, u32) -> u32> = Function::new("rust", "run");
    let mut program = Program::new();
    let accept = ACCEPT.import(&mut program);
    RUN.define(&mut program, move |body, (first, second)| {
        accept.tail_call(body, (first, second))
    })
    .unwrap();
    let mut backend = Backend::with_state(0_u32).unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let imports = [backend.bind_host(
        ACCEPT,
        Box::new(|mut context, first, second| {
            assert_eq!(LIVE_ARGUMENTS.load(Ordering::SeqCst), 2);
            assert_eq!((first.ordinal, second.ordinal), (0, 1));
            *context.state_mut() += 1;
            *context.state()
        }),
    )];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    for calls in 1..=2 {
        assert_eq!(backend.call(&run, (0, 0)), calls);
        assert_eq!(LIVE_ARGUMENTS.load(Ordering::SeqCst), 0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_decode_in_order_and_release_owned_arguments() {
    pollster::block_on(check_callback_arguments());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_decode_in_order_and_release_owned_arguments() {
    check_callback_arguments().await;
}
