#![cfg(target_arch = "wasm32")]

use relocade_compiler::{
    abi::{AbiType, AbiValue, Carrier, Function},
    Program, I64,
};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};
use std::cell::{Cell, RefCell};

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_keep_backend_state_after_the_original_backend_is_dropped() {
    type Count = fn(u32) -> u32;
    const COUNT: Function<Count> = Function::new("direct", "count");
    const RUN: Function<Count> = Function::new("direct", "run");
    let import = {
        let mut owner = Backend::with_state(40_u32).unwrap();
        owner.bind_host(
            COUNT,
            Box::new(|mut context, amount| {
                *context.state_mut() += amount;
                *context.state()
            }),
        )
    };
    let mut backend = Backend::with_state(0_u32).unwrap();
    let mut program = Program::new();
    let count = COUNT.import(&mut program);
    RUN.define(&mut program, move |body, (amount,)| {
        count.tail_call(body, amount)
    })
    .unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let instance = backend.instantiate(&module, &[import]).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    assert_eq!(backend.call(&run, (1,)), 41);
    assert_eq!(backend.call(&run, (2,)), 43);
    assert_eq!(*backend.state(), 0);
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    Lift,
    Callback,
    Lower,
}
type Exchange = fn(u64, u64) -> (u64, u64);
type Run = relocade_wasm_runtime::Function<Backend, Exchange>;
thread_local! {
    static NESTED: RefCell<Option<(Backend, Run)>> = const { RefCell::new(None) };
    static PHASE: Cell<Phase> = const { Cell::new(Phase::Idle) };
    static INSIDE: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<u32> = const { Cell::new(0) };
}

fn reenter(phase: Phase) {
    if PHASE.with(Cell::get) != phase || INSIDE.with(|inside| inside.replace(true)) {
        return;
    }
    NESTED.with(|nested| {
        let mut nested = nested.borrow_mut();
        let (backend, run) = nested.as_mut().unwrap();
        assert_eq!(
            backend.call(run, (u64::MAX, 0x8000_0000_0000_0000)),
            (0, 0xff_0000_0000_0001)
        );
    });
    CALLS.with(|calls| calls.set(calls.get() + 1));
    INSIDE.with(|inside| inside.set(false));
}

struct Reentrant(u64);
impl AbiType for Reentrant {
    type Logical = I64;
}

impl AbiValue for Reentrant {
    type Value = Self;
    fn lower(value: Self) -> Carrier<Self> {
        reenter(Phase::Lower);
        u64::lower(value.0)
    }
    fn lift(value: Carrier<Self>) -> Option<Self> {
        reenter(Phase::Lift);
        Some(Self(u64::lift(value)?))
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn codecs_and_callbacks_can_reenter_the_same_binding() {
    type ReentrantExchange = fn(Reentrant, u64) -> (Reentrant, u64);
    const HOST: Function<ReentrantExchange> = Function::new("direct", "host");
    const RUN: Function<Exchange> = Function::new("direct", "run");
    let mut backend = Backend::new().unwrap();
    let mut program = Program::new();
    let host = HOST.import(&mut program);
    RUN.define(&mut program, move |body, (first, second)| {
        host.tail_call(body, (first, second))
    })
    .unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let imports = [backend.bind_host(
        HOST,
        Box::new(|context, first, second| {
            // Reentry needs a fresh scoped borrow of the same callback state.
            drop(context);
            reenter(Phase::Callback);
            (
                Reentrant(first.0.wrapping_add(1)),
                second ^ 0x80ff_0000_0000_0001,
            )
        }),
    )];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    let mut nested = Backend::new().unwrap();
    let nested_instance = nested.instantiate(&module, &imports).await.unwrap();
    let nested_run = nested.function(&nested_instance, RUN).unwrap();
    NESTED.with(|slot| *slot.borrow_mut() = Some((nested, nested_run)));
    for phase in [Phase::Lift, Phase::Callback, Phase::Lower] {
        PHASE.with(|slot| slot.set(phase));
        assert_eq!(
            backend.call(&run, (42, 0x0123_4567_89ab_cdef)),
            (43, 0x81dc_4567_89ab_cdee)
        );
    }
    PHASE.with(|slot| slot.set(Phase::Idle));
    NESTED.with(|slot| slot.borrow_mut().take());
    assert_eq!(CALLS.with(Cell::get), 3);
}
