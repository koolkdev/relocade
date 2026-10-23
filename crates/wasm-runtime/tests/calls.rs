use relocade_compiler::{
    abi::{AbiType, AbiValue, Carrier, Function, Table},
    Program, I32, I64,
};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_rust_calls() {
    type Narrow = fn(bool, i8, i16, u32) -> (bool, i8, i16, u32);
    const NARROW: Function<Narrow> = Function::new("rust", "narrow");
    const ADD: Function<fn(u16, i16) -> i32> = Function::new("rust", "add");
    const THREE: Function<fn(u32, u32, u32) -> u32> = Function::new("rust", "three");
    const WIDE: Function<fn(u64) -> u64> = Function::new("rust", "wide");
    const SIGNED: Function<fn(i64) -> i64> = Function::new("rust", "signed");
    const FLOAT: Function<fn(f64) -> f64> = Function::new("rust", "float");
    const SINGLE: Function<fn() -> (i64,)> = Function::new("rust", "single");
    const OBSERVE: Function<fn(u32)> = Function::new("rust", "observe");
    const CONSUME: Function<fn(u32)> = Function::new("rust", "consume");
    let mut program = Program::new();
    NARROW
        .define(&mut program, move |body, (bit, byte, word, integer)| {
            body.return_((bit.eq(false), byte.add(1), word.add(1), integer.add(1)))
        })
        .unwrap();
    ADD.define(&mut program, move |body, (a, b)| {
        body.return_(a.unsigned().extend::<I32>().add(b.signed().extend::<I32>()))
    })
    .unwrap();
    THREE
        .define(&mut program, move |body, (a, b, c)| {
            body.return_(a.add(b).add(c))
        })
        .unwrap();
    WIDE.define(&mut program, move |body, (value,)| {
        body.return_(value.add(1))
    })
    .unwrap();
    SIGNED
        .define(&mut program, move |body, (value,)| body.return_(value))
        .unwrap();
    FLOAT
        .define(&mut program, move |body, (value,)| body.return_(value))
        .unwrap();
    SINGLE
        .define(&mut program, move |body, ()| {
            body.return_((0x20_0000_0000_0001_u64,))
        })
        .unwrap();
    let observe = OBSERVE.import(&mut program);
    CONSUME
        .define(&mut program, move |body, (value,)| {
            observe.tail_call(body, value)
        })
        .unwrap();

    let mut backend = Backend::with_state(0_u32).unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let imports = [backend.bind_host(
        OBSERVE,
        Box::new(|mut context, value| *context.state_mut() = value),
    )];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let narrow = backend.function(&instance, NARROW).unwrap();
    assert_eq!(
        backend.call(&narrow, (true, i8::MAX, i16::MAX, u32::MAX)),
        (false, i8::MIN, i16::MIN, 0)
    );
    assert_eq!(
        backend.call(&narrow, (false, -1, -1, 0x8000_0000)),
        (true, 0, 0, 0x8000_0001)
    );
    let add = backend.function(&instance, ADD).unwrap();
    assert_eq!(backend.call(&add, (u16::MAX, i16::MIN)), 32767);
    let three = backend.function(&instance, THREE).unwrap();
    assert_eq!(backend.call(&three, (10, 20, 12)), 42);
    let wide = backend.function(&instance, WIDE).unwrap();
    assert_eq!(backend.call(&wide, (u64::MAX,)), 0);
    assert_eq!(
        backend.call(&wide, (0x8000_0000_0000_0001,)),
        0x8000_0000_0000_0002
    );
    let signed = backend.function(&instance, SIGNED).unwrap();
    for value in [
        i64::MIN,
        i64::MAX,
        -0x20_0000_0000_0001,
        0x20_0000_0000_0001,
    ] {
        assert_eq!(backend.call(&signed, (value,)), value);
    }
    let float = backend.function(&instance, FLOAT).unwrap();
    for value in [-0.0_f64, -1.75, f64::INFINITY] {
        assert_eq!(backend.call(&float, (value,)).to_bits(), value.to_bits());
    }
    assert!(backend.call(&float, (f64::NAN,)).is_nan());
    let single = backend.function(&instance, SINGLE).unwrap();
    let consume = backend.function(&instance, CONSUME).unwrap();
    assert!(backend
        .function(&instance, Function::<fn() -> i64>::new("rust", "missing"))
        .is_err());
    for _ in 0..2 {
        assert_eq!(backend.call(&single, ()), (0x20_0000_0000_0001,));
        backend.call(&consume, (u32::MAX,));
        assert_eq!(*backend.state(), u32::MAX);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cached_exports_use_rust_arguments_and_results() {
    pollster::block_on(check_rust_calls());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cached_exports_use_rust_arguments_and_results() {
    check_rust_calls().await;
}

async fn check_typed_linking() {
    type Query = fn(u16) -> (bool, u16, u64, f64);
    const QUERY: Function<Query> = Function::new("typed", "query");
    const RELAY: Function<Query> = Function::new("typed", "relay");
    const TABLE: Table<Query> = Table::new("typed", "table", 1);
    let mut backend = Backend::new().unwrap();
    let table = backend.create_table(1).unwrap();
    let query = {
        let mut producer = Program::new();
        QUERY
            .define(&mut producer, move |body, (value,)| {
                body.return_((
                    value.eq(0),
                    value.add(1),
                    value
                        .unsigned()
                        .extend::<I64>()
                        .or(0x8000_0000_0000_0000_u64),
                    -0.0_f64,
                ))
            })
            .unwrap();
        let module = backend
            .compiler()
            .compile(&producer.compile().unwrap())
            .await
            .unwrap();
        let instance = backend.instantiate(&module, &[]).await.unwrap();
        backend.function(&instance, QUERY).unwrap()
    };
    backend.set_table_function(&table, 0, &query).unwrap();
    let mut consumer = Program::new();
    let direct = QUERY.import(&mut consumer);
    let indirect = consumer.import_table(TABLE);
    RELAY
        .define(&mut consumer, move |mut body, (value,)| {
            let (_, next, _, _) = direct.call(&mut body, value)?;
            body.tail_call_indirect(indirect, 0, &[next.into()])
        })
        .unwrap();
    let module = backend
        .compiler()
        .compile(&consumer.compile().unwrap())
        .await
        .unwrap();
    let instance = backend
        .instantiate(
            &module,
            &[backend.bind(QUERY, &query), backend.bind(TABLE, &table)],
        )
        .await
        .unwrap();
    let relay = backend.function(&instance, RELAY).unwrap();
    for (argument, expected) in [
        (u16::MAX, (true, 1, 0x8000_0000_0000_0000)),
        (0x8123, (false, 0x8125, 0x8000_0000_0000_8124)),
    ] {
        let (bit, word, wide, float) = backend.call(&relay, (argument,));
        assert_eq!((bit, word, wide), expected);
        assert_eq!(float.to_bits(), (-0.0_f64).to_bits());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn typed_exports_link_through_imports_and_tables() {
    pollster::block_on(check_typed_linking());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn typed_exports_link_through_imports_and_tables() {
    check_typed_linking().await;
}

#[derive(Debug, PartialEq)]
enum Mode {
    Read,
    Write,
}

impl AbiType for Mode {
    type Logical = I32;
}

impl AbiValue for Mode {
    type Value = Self;
    fn lower(value: Self) -> Carrier<Self> {
        u32::lower(match value {
            Self::Read => 0,
            Self::Write => 1,
        })
    }
    fn lift(value: Carrier<Self>) -> Option<Self> {
        match u32::lift(value)? {
            0 => Some(Self::Read),
            1 => Some(Self::Write),
            _ => None,
        }
    }
}

async fn check_domain_values() {
    const NEXT: Function<fn(Mode) -> Mode> = Function::new("typed", "next");
    let mut program = Program::new();
    Function::<fn(u32) -> u32>::new("typed", "next")
        .define(&mut program, move |body, (value,)| {
            body.return_(value.add(1))
        })
        .unwrap();
    let mut backend = Backend::new().unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let instance = backend.instantiate(&module, &[]).await.unwrap();
    let next = backend.function(&instance, NEXT).unwrap();
    assert_eq!(backend.call(&next, (Mode::Read,)), Mode::Write);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn domain_values_use_their_shared_codec() {
    pollster::block_on(check_domain_values());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn domain_values_use_their_shared_codec() {
    check_domain_values().await;
}
