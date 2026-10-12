use std::sync::OnceLock;
use wasm86_runtime::{Machine, Runtime};
use wasm86_runtime_wasmtime::WasmtimeBackend;
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};

#[path = "runtime/devices.rs"]
mod devices;
#[path = "runtime/execution.rs"]
mod execution;
#[path = "runtime/memory.rs"]
mod memory;

type TestBackend = WasmtimeBackend;
type TestMachine<D = ()> = Machine<TestBackend, D>;

async fn runtime() -> Runtime<TestBackend> {
    static RUNTIME: OnceLock<Runtime<TestBackend>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| pollster::block_on(Runtime::new(WasmtimeBackend::new().unwrap())).unwrap())
        .clone()
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
