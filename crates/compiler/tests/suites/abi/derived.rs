use crate::wasm::TestModule;
use relocade_compiler::{
    abi::{
        slots::{Layout, Tuple},
        AbiValue, CompilerValue, Function,
    },
    Program, Type, V128,
};
use relocade_wasm_bindings::WasmAbi;

#[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
struct Header<T = u16> {
    code: u8,
    value: T,
    enabled: bool,
}

#[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
struct Packet {
    header: Header,
    weight: f64,
}

#[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
struct Pair(u16, u64);

#[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
struct Empty;

#[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
struct Batch<T, const N: usize> {
    items: [T; N],
}

#[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
enum Outcome {
    Ready,
    Value(u32),
    Pair { value: u16, enabled: bool },
    Weight(f64),
    Packet(Packet),
}

#[test]
fn compiler_only_aggregates_cross_typed_parameters_calls_and_returns() {
    type Echo = fn(Header<V128>, Batch<u16, 16>) -> (Header<V128>, Batch<u16, 16>);
    const ECHO: Function<Echo> = Function::new("abi", "echo");
    const RUN: Function<fn() -> u32> = Function::new("abi", "run");
    let signature = ECHO.signature();
    assert_eq!(signature.parameters, signature.results);
    assert_eq!(
        &signature.parameters[..3],
        &[Type::I8, Type::V128, Type::I1]
    );
    assert_eq!(&signature.parameters[3..], &[Type::I16; 16]);
    let mut program = Program::new();
    let echo = program
        .function::<Echo>(|body, (header, batch)| body.return_((header, batch)))
        .unwrap();
    RUN.define(&mut program, |mut body, ()| {
        let header = HeaderValues::<V128> {
            code: 7.into(),
            value: 0x1234_u128.into(),
            enabled: true.into(),
        };
        let batch = BatchValues::<u16, 16> {
            items: std::array::from_fn::<_, 16, _>(|index| (index as u32).into()),
        };
        let (header, batch) = echo.call(&mut body, (header, batch))?;
        body.return_(
            header
                .value
                .extract_lane::<relocade_compiler::I32>(0)
                .add(header.code.unsigned().extend::<relocade_compiler::I32>())
                .add(
                    batch.items[15]
                        .unsigned()
                        .extend::<relocade_compiler::I32>(),
                ),
        )
    })
    .unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    assert_eq!(
        module.instantiate().call::<i32>(()).unwrap(),
        0x1234 + 7 + 15
    );
}

#[test]
fn const_generic_fields_cross_generated_function_boundaries() {
    type Pairs = Batch<(i8, bool), 2>;
    const ECHO: Function<fn(Pairs) -> Pairs> = Function::new("abi", "run");
    let mut program = Program::new();
    ECHO.define(&mut program, |body, (batch,)| body.return_(batch))
        .unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    let mut instance = module.instantiate();
    let flat = instance
        .call::<(i32, i32, i32, i32)>((255, 1, 128, 0))
        .unwrap();
    assert_eq!(
        Pairs::lift(<Layout<Pairs> as Tuple>::from_tuple(flat)),
        Some(Batch {
            items: [(-1, true), (i8::MIN, false)]
        })
    );
}

#[test]
fn derived_fields_flatten_in_declaration_order() {
    const PACKET: Function<fn(Packet, Pair, Empty) -> Header> = Function::new("abi", "packet");
    let signature = PACKET.signature();
    assert_eq!(
        signature.parameters,
        [
            Type::I8,
            Type::I16,
            Type::I1,
            Type::F64,
            Type::I16,
            Type::I64
        ]
    );
    assert_eq!(signature.results, [Type::I8, Type::I16, Type::I1]);
}

#[test]
fn enum_signatures_use_the_shared_payload_layout() {
    const OUTCOME: Function<fn() -> Outcome> = Function::new("abi", "outcome");
    assert_eq!(
        OUTCOME.signature().results,
        [Type::I32, Type::I64, Type::I16, Type::I1, Type::F64]
    );
}

#[test]
fn generated_enum_matching_projects_only_the_active_variant() {
    const RUN: Function<fn(Outcome) -> u32> = Function::new("abi", "run");
    let mut program = Program::new();
    RUN.define(&mut program, |mut body, (outcome,)| {
        outcome.match_(&mut body, |arm, variant| match variant {
            OutcomeCase::Ready => arm.return_(10),
            OutcomeCase::Value(value) => arm.return_(value.add(1)),
            OutcomeCase::Pair { value, enabled } => arm.return_(
                value
                    .unsigned()
                    .extend::<relocade_compiler::I32>()
                    .add(enabled.unsigned().extend::<relocade_compiler::I32>()),
            ),
            OutcomeCase::Weight(value) => {
                arm.return_(value.to_bits().truncate::<relocade_compiler::I32>())
            }
            OutcomeCase::Packet(packet) => arm.return_(
                packet
                    .header
                    .value
                    .unsigned()
                    .extend::<relocade_compiler::I32>(),
            ),
        })?;
        body.trap()
    })
    .unwrap();
    let bytes = program.compile().unwrap();
    let module = TestModule::new(&bytes);
    let mut instance = module.instantiate();
    assert_eq!(
        instance
            .call::<i32>((0_i32, 0_i64, 0_i32, 0_i32, 0.0_f64))
            .unwrap(),
        10
    );
    assert_eq!(
        instance
            .call::<i32>((1_i32, 41_i64, 0_i32, 0_i32, 0.0_f64))
            .unwrap(),
        42
    );
    assert_eq!(
        instance
            .call::<i32>((2_i32, 65535_i64, 1_i32, 0_i32, 0.0_f64))
            .unwrap(),
        65536
    );
    assert_eq!(
        instance
            .call::<i32>((3_i32, 0x3ff0_0000_1234_5678_i64, 0_i32, 0_i32, 0.0_f64))
            .unwrap(),
        0x1234_5678
    );
    assert!(instance
        .call::<i32>((99_i32, 0_i64, 0_i32, 0_i32, 0.0_f64))
        .is_err());
}

#[test]
fn compiler_enum_constructors_and_matching_use_the_declared_abi_tags() {
    use relocade_wasm_bindings::{slots::Cons, AbiType, Carrier, Enum, I32, I8};

    #[derive(CompilerValue)]
    enum Reply {
        Missing,
        Found(u8),
    }
    impl AbiType for Reply {
        type Logical = (I32, Cons<I8, ()>);
    }
    impl Enum for Reply {
        type Payload = Cons<I8, ()>;
        const TAGS: &'static [u32] = &[7, 42];
    }
    impl AbiValue for Reply {
        type Value = Self;
        fn lower(value: Self) -> Carrier<Self> {
            match value {
                Self::Missing => (7, (0, ())),
                Self::Found(value) => (42, (i32::from(value), ())),
            }
        }
        fn lift(value: Carrier<Self>) -> Option<Self> {
            match value {
                (7, _) => Some(Self::Missing),
                (42, (value, ())) => Some(Self::Found(value as u8)),
                _ => None,
            }
        }
    }

    const RUN: Function<fn(Reply) -> Reply> = Function::new("abi", "run");
    let mut program = Program::new();
    RUN.define(&mut program, |mut body, (reply,)| {
        reply.match_(&mut body, |arm, case| match case {
            ReplyCase::Missing => arm.return_(ReplyValues::found(255)),
            ReplyCase::Found(_) => arm.return_(ReplyValues::missing()),
        })?;
        body.trap()
    })
    .unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<(i32, i32)>((7, 0)).unwrap(), (42, 255));
    assert_eq!(instance.call::<(i32, i32)>((42, 127)).unwrap(), (7, 0));
    assert!(instance.call::<(i32, i32)>((0, 0)).is_err());
}

mod hygiene {
    use relocade_compiler::abi::{AbiValue, CompilerValue};
    use relocade_wasm_bindings::WasmAbi;
    use Reply::*;

    type Result<T> = std::result::Result<T, ()>;

    #[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
    enum Reply {
        None,
        Some(u32),
    }

    #[derive(WasmAbi, CompilerValue, Debug, PartialEq)]
    struct Record {
        reply: Reply,
    }

    #[derive(WasmAbi, CompilerValue)]
    enum Constructors {
        Type(u32),
        Clone(u32),
        From(u32),
        Crate(u32),
    }

    #[test]
    fn keyword_constructors_coexist_with_clone_and_native_conversion() {
        use relocade_compiler::{abi::Function, Program};
        let mut program = Program::new();
        let read = Function::<fn(Constructors) -> u32>::new("abi", "read")
            .define(&mut program, |mut body, (value,)| {
                value.match_(&mut body, |arm, case| match case {
                    ConstructorsCase::Type(value)
                    | ConstructorsCase::Clone(value)
                    | ConstructorsCase::From(value)
                    | ConstructorsCase::Crate(value) => arm.return_(value),
                })?;
                body.trap()
            })
            .unwrap();
        Function::<fn() -> u32>::new("abi", "run")
            .define(&mut program, |mut body, ()| {
                let cloned = ConstructorsValues::clone(11);
                let values = [
                    ConstructorsValues::r#type(7),
                    cloned.clone(),
                    ConstructorsValues::from(13),
                    ConstructorsValues::crate_(17),
                    <ConstructorsValues as From<Constructors>>::from(Constructors::From(23)),
                ];
                let mut sum = relocade_compiler::Val::<relocade_compiler::I32>::from(0);
                for value in values {
                    sum = sum.add(read.call(&mut body, value)?);
                }
                body.return_(sum)
            })
            .unwrap();
        let module = crate::wasm::TestModule::new(&program.compile().unwrap());
        assert_eq!(module.instantiate().call::<i32>(()).unwrap(), 71);
    }

    #[test]
    fn derives_do_not_capture_local_result_and_variant_names() -> Result<()> {
        let record = Record { reply: Some(7) };
        assert_eq!(
            Record::lift(Record::lower(record)),
            std::option::Option::Some(Record { reply: Some(7) })
        );
        assert_eq!(
            Reply::lift(Reply::lower(None)),
            std::option::Option::Some(None)
        );
        let _ = ReplyValues::from(Some(9));
        Ok(())
    }
}

#[test]
fn enum_constructors_return_named_variants_through_generated_calls() {
    const MAKE: Function<fn(u16, bool) -> Outcome> = Function::new("abi", "make");
    const RUN: Function<fn(u16, bool) -> Outcome> = Function::new("abi", "run");
    let mut program = Program::new();
    let make = MAKE
        .define(&mut program, |body, (value, enabled)| {
            body.return_(OutcomeValues::pair(value.add(1), enabled))
        })
        .unwrap();
    RUN.define(&mut program, |mut body, arguments| {
        let outcome = make.call(&mut body, arguments)?;
        body.return_(outcome)
    })
    .unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    let flat = module
        .instantiate()
        .call::<(i32, i64, i32, i32, f64)>((41, 1))
        .unwrap();
    assert_eq!(flat, (2, 42, 1, 0, 0.0));
}
