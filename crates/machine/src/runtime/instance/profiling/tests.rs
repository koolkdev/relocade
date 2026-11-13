use super::*;
use relocade_compiler::{abi::Function as FunctionDeclaration, Program};
use relocade_wasm_runtime::{Function, WasmCompiler};

struct Recorder {
    wasm: WasmRuntime,
    profiling: Profiling,
    record: Function<WasmRuntime, fn(u32) -> bool>,
}

impl Recorder {
    async fn new() -> Self {
        let mut wasm = WasmRuntime::new().unwrap();
        let profiling = Profiling::new(&mut wasm).unwrap();
        let mut program = Program::new();
        let record = CallProfile::declare_recorder(&mut program);
        let declaration = FunctionDeclaration::<fn(u32) -> bool>::new("", "record");
        declaration.export(&mut program, record).unwrap();
        let module = wasm
            .compiler()
            .compile(&program.compile().unwrap())
            .await
            .unwrap();
        let instance = wasm
            .instantiate(&module, &[wasm.bind(abi::CALL_PROFILE, &profiling.memory)])
            .unwrap();
        let record = wasm.function(&instance, declaration).unwrap();
        Self {
            wasm,
            profiling,
            record,
        }
    }

    fn record(&mut self, target: u32) -> bool {
        self.record.call(&mut self.wasm, (target,))
    }

    fn finish(&mut self, exit: ExecutionExit) -> ExecutionTurn {
        self.profiling.finish(&mut self.wasm, exit)
    }

    fn count(&self, key: u32) -> u64 {
        self.profiling.count(FarPointer::from_packed(key))
    }
}

async fn pressure() {
    let mut recorder = Recorder::new().await;
    // Ordinary stops retain a small batch without merging or clearing it.
    assert!(recorder.record(0));
    assert!(matches!(
        recorder.finish(ExecutionExit::StepComplete),
        ExecutionTurn::Exit(ExecutionExit::StepComplete)
    ));
    assert_eq!(recorder.count(0), 0);
    assert!(recorder.record(0));
    assert!(matches!(
        recorder.finish(ExecutionExit::Interrupted),
        ExecutionTurn::Exit(ExecutionExit::Interrupted)
    ));
    assert_eq!(recorder.count(0), 0);
    recorder.profiling.collect(&mut recorder.wasm);
    assert_eq!(recorder.count(0), 2);

    // An interrupt can hide the full-profile exit. The overflow marker must
    // still cause a drain, leaving the buffer usable at the next guest entry.
    let full_at = (1..=513)
        .find(|&target| !recorder.record(target))
        .expect("bounded collector fills");
    assert!(matches!(
        recorder.finish(ExecutionExit::Interrupted),
        ExecutionTurn::Exit(ExecutionExit::Interrupted)
    ));
    for target in 1..=full_at {
        assert_eq!(recorder.count(target), 1);
    }
    assert!(recorder.record(full_at));
    recorder.profiling.collect(&mut recorder.wasm);
    assert_eq!(recorder.count(full_at), 2);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ordinary_stops_retain_batches_and_masked_overflow_drains() {
    pollster::block_on(pressure());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn ordinary_stops_retain_batches_and_masked_overflow_drains() {
    pressure().await;
}

async fn periodic_collection() {
    use std::{
        num::NonZeroU64,
        task::{Context, Waker},
        time::Duration,
    };
    let mut recorder = Recorder::new().await;
    let time = crate::GuestTimeDriver::new(NonZeroU64::new(1000).unwrap());
    let clock = time.clock();
    let start = clock.now();
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..5 {
        assert!(recorder.record(42));
    }
    recorder.profiling.poll(&mut recorder.wasm, &clock, &mut cx);
    let first = clock.add(start, Duration::from_millis(10));
    assert_eq!(time.next_deadline(), Some(first));
    time.advance_to(clock.add(start, Duration::from_millis(9)));
    recorder.profiling.poll(&mut recorder.wasm, &clock, &mut cx);
    assert_eq!(recorder.count(42), 0);
    for _ in 0..7 {
        assert!(recorder.record(42));
    }
    time.advance_to(first);
    recorder.profiling.poll(&mut recorder.wasm, &clock, &mut cx);
    assert_eq!(recorder.count(42), 12);
    let next = clock.add(start, Duration::from_millis(20));
    assert_eq!(time.next_deadline(), Some(next));
    time.advance_to(next);
    recorder.profiling.poll(&mut recorder.wasm, &clock, &mut cx);
    assert_eq!(recorder.count(42), 12); // The next empty batch adds nothing.
    assert_eq!(
        time.next_deadline(),
        Some(clock.add(start, Duration::from_millis(30)))
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn small_batches_wait_for_the_sampling_deadline() {
    pollster::block_on(periodic_collection());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn small_batches_wait_for_the_sampling_deadline() {
    periodic_collection().await;
}
