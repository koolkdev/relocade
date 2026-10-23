use relocade_compiler::{
    abi::{CompilerValue, Function},
    Program,
};
use relocade_wasm_bindings::WasmAbi;
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(WasmAbi, CompilerValue, Clone, Debug, PartialEq)]
struct Request {
    key: u32,
    location: (u64, f64),
    enabled: bool,
}

#[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
enum Reply {
    Empty,
    Data { total: u64, accepted: bool },
    Sample(f64),
}

async fn check_aggregates() {
    type Exchange = fn(Request, [u16; 4]) -> Reply;
    const EXCHANGE: Function<Exchange> = Function::new("aggregate", "exchange");
    const RUN: Function<Exchange> = Function::new("aggregate", "run");
    let mut program = Program::new();
    let exchange = EXCHANGE.import(&mut program);
    RUN.define(&mut program, move |mut body, (mut request, counts)| {
        request.key = request.key.add(1);
        let reply = exchange.call(&mut body, (request, counts))?;
        reply.match_(&mut body, |arm, variant| {
            let result = match variant {
                ReplyCase::Empty => ReplyValues::empty(),
                ReplyCase::Data { total, accepted } => ReplyValues::data(total.add(1), accepted),
                ReplyCase::Sample(value) => ReplyValues::sample(value),
            };
            arm.return_(result)
        })?;
        body.trap()
    })
    .unwrap();
    let mut backend = Backend::with_state(0_u32).unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let imports = [backend.bind_host(
        EXCHANGE,
        Box::new(|mut context, request, counts| {
            assert_eq!(counts, [0, 1, 0x8000, u16::MAX]);
            assert_eq!(request.location.0, 0x8000_0000_0000_0001);
            assert!(request.enabled);
            assert_eq!(request.location.1.to_bits(), (-0.0_f64).to_bits());
            *context.state_mut() += 1;
            match request.key {
                1 => Reply::Empty,
                2 => Reply::Data {
                    total: u64::MAX - 1,
                    accepted: request.enabled,
                },
                3 => Reply::Sample(f64::from_bits(0xfff8_1234_5678_9abc)),
                _ => unreachable!(),
            }
        }),
    )];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    let nan = f64::from_bits(0xfff8_1234_5678_9abc);
    let request = Request {
        key: 0,
        location: (0x8000_0000_0000_0001, -0.0),
        enabled: true,
    };
    let counts = [0, 1, 0x8000, u16::MAX];
    assert_eq!(backend.call(&run, (request.clone(), counts)), Reply::Empty);
    assert_eq!(
        backend.call(
            &run,
            (
                Request {
                    key: 1,
                    ..request.clone()
                },
                counts
            )
        ),
        Reply::Data {
            total: u64::MAX,
            accepted: true
        }
    );
    let Reply::Sample(value) = backend.call(&run, (Request { key: 2, ..request }, counts)) else {
        panic!("sample variant");
    };
    assert_eq!(value.to_bits(), nan.to_bits());
    assert_eq!(*backend.state(), 3);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn aggregates_cross_imports_and_exports_with_eight_flat_parameters() {
    pollster::block_on(check_aggregates());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn aggregates_cross_imports_and_exports_with_eight_flat_parameters() {
    check_aggregates().await;
}
