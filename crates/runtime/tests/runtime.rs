use wasm86_runtime::{Machine, Runtime};
#[cfg(not(target_arch = "wasm32"))]
use wasm86_runtime_wasmtime::WasmtimeBackend as TestBackend;
#[cfg(target_arch = "wasm32")]
use wasm86_runtime_web::WebBackend as TestBackend;
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};

#[path = "runtime/devices.rs"]
mod devices;
#[path = "runtime/dispatch.rs"]
mod dispatch;
#[path = "runtime/execution.rs"]
mod execution;
#[path = "runtime/hooks.rs"]
mod hooks;
#[path = "runtime/memory.rs"]
mod memory;
#[path = "runtime/run.rs"]
mod run;
#[path = "runtime/slices.rs"]
mod slices;

type TestMachine<D = ()> = Machine<TestBackend, D>;

#[cfg(not(target_arch = "wasm32"))]
async fn runtime() -> Runtime<TestBackend> {
    use std::sync::OnceLock;
    static RUNTIME: OnceLock<Runtime<TestBackend>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| pollster::block_on(Runtime::new(TestBackend::new().unwrap())).unwrap())
        .clone()
}

#[cfg(target_arch = "wasm32")]
async fn runtime() -> Runtime<TestBackend> {
    use std::cell::RefCell;
    thread_local! {
        static RUNTIME: RefCell<Option<Runtime<TestBackend>>> = const { RefCell::new(None) };
    }
    if let Some(runtime) = RUNTIME.with_borrow(Clone::clone) {
        return runtime;
    }
    let runtime = Runtime::new(TestBackend).await.unwrap();
    RUNTIME.with_borrow_mut(|cache| *cache = Some(runtime.clone()));
    runtime
}

fn ram() -> PhysicalMemoryMap {
    PhysicalMemoryMap::new([(0..=0x10ffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap()
}

async fn machine(code: &[u8]) -> TestMachine {
    let mut machine = runtime().await.machine(ram(), ()).await.unwrap();
    machine.write_memory(0x100, code).unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    cpu.registers.esp = 0xff00;
    machine.set_cpu(cpu).unwrap();
    machine
}
