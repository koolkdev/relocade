mod execution;

use super::*;
use crate::{Cpu, FarPointer};
use relocade_compiler::{abi::Function as FunctionDeclaration, Program};
use relocade_wasm_runtime::{Function, WasmCompiler};

fn merged_count(profiling: &Profiling, target: u32) -> u64 {
    profiling.counts.get(&target).copied().unwrap_or(0)
}

fn collected_count(cpu: &mut Cpu, target: FarPointer) -> u64 {
    let instance = &mut cpu.instance;
    instance
        .runtime
        .with_runtime(|wasm| instance.profiling.collect(wasm));
    merged_count(&instance.profiling, target.packed())
}

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

    fn count(&self, key: u32) -> u64 {
        merged_count(&self.profiling, key)
    }
}

async fn collection() {
    let mut recorder = Recorder::new().await;
    assert!(recorder.record(0));
    assert!(recorder.record(42));
    assert!(recorder.record(42));
    assert_eq!(recorder.count(0), 0);
    assert_eq!(recorder.count(42), 0);
    recorder.profiling.collect(&mut recorder.wasm);
    assert_eq!(recorder.count(0), 1);
    assert_eq!(recorder.count(42), 2);
    assert!(recorder.record(0));
    assert!(recorder.record(42));
    recorder.profiling.collect(&mut recorder.wasm);
    assert_eq!(recorder.count(0), 2);
    assert_eq!(recorder.count(42), 3);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn collection_merges_batches_without_resetting_totals() {
    pollster::block_on(collection());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn collection_merges_batches_without_resetting_totals() {
    collection().await;
}
