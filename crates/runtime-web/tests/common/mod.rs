#![allow(dead_code)]

use relocade_machine::MachineConfig;
use relocade_runtime::{Runtime, RuntimeState};
use relocade_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};
use std::{cell::Cell, rc::Rc};

#[cfg(not(target_arch = "wasm32"))]
pub use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
pub use relocade_runtime_web::WebBackend as Backend;

pub type TestState = Rc<Cell<u32>>;
pub type TestRuntime = Runtime<Backend<RuntimeState<TestState>>>;

pub struct HeldCompiler;
impl<M: 'static> relocade_runtime::compilation::BackgroundCompiler<M> for HeldCompiler {
    fn submit(
        &mut self,
        _snapshot: relocade_runtime::CodeSnapshot,
    ) -> relocade_runtime::Result<Option<relocade_runtime::compilation::Compilation<M>>> {
        Ok(Some(Box::pin(std::future::pending())))
    }
}

pub async fn runtime() -> TestRuntime {
    let state = RuntimeState::new(Rc::new(Cell::new(0)));
    #[cfg(not(target_arch = "wasm32"))]
    let backend = Backend::with_state(state).unwrap();
    #[cfg(target_arch = "wasm32")]
    let backend = Backend::with_state(state);
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x1000,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([
            (0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 }),
            (
                0x2000..=0x2fff,
                PhysicalMapping::Ram {
                    backing_offset: 0x1000,
                },
            ),
        ])
        .unwrap(),
    )
    .unwrap();
    let runtime = Runtime::new(backend, config).await.unwrap();
    #[cfg(target_arch = "wasm32")]
    let runtime = {
        let mut runtime = runtime;
        runtime.set_compiler(Some(Box::new(
            relocade_runtime_web::WebCompiler::new(
                &test_url("/wasm-bindgen-test"),
                &test_url("/wasm-bindgen-test_bg.wasm"),
            )
            .unwrap(),
        )));
        runtime
    };
    runtime
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function test_url(path) { return new URL(path, location.href).href; }
export function next_task() { return new Promise(resolve => setTimeout(resolve, 0)); }
")]
extern "C" {
    pub fn test_url(path: &str) -> String;
    fn next_task() -> js_sys::Promise;
}

pub async fn yield_host() {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::JsFuture::from(next_task())
        .await
        .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(std::time::Duration::from_millis(1));
}

pub async fn complete(runtime: &mut TestRuntime) -> usize {
    let mut published = 0;
    for _ in 0..20_000 {
        published += runtime.publish_ready_groups().await.unwrap();
        if runtime.pending_compilation_count() == 0 {
            return published;
        }
        yield_host().await;
    }
    panic!("compiler did not finish");
}
