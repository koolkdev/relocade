#![cfg(not(target_arch = "wasm32"))]

use wasm86_machine::{Environment, Machine};
use wasm86_runtime::{Runtime, Session};
use wasm86_runtime_wasmtime::WasmtimeBackend as TestBackend;
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};

#[path = "runtime/budget.rs"]
mod budget;
#[path = "runtime/configurations.rs"]
mod configurations;
#[path = "runtime/execution.rs"]
mod execution;

#[path = "runtime/devices.rs"]
mod devices;
#[path = "runtime/memory.rs"]
mod memory;

type TestMachine<D = ()> = Session<TestBackend, D>;

async fn runtime() -> Runtime<TestBackend> {
    use std::cell::RefCell;
    thread_local! {
        static RUNTIME: RefCell<Option<Runtime<TestBackend>>> = const { RefCell::new(None) };
    }
    RUNTIME.with_borrow_mut(|cache| {
        cache
            .get_or_insert_with(|| {
                let backend = TestBackend::new().unwrap();
                Runtime::new(backend)
            })
            .clone()
    })
}

fn real_machine<E: Environment>(map: PhysicalMemoryMap, environment: E) -> Machine<E> {
    use wasm86_x86::{Compiler, CpuState, ExecutionProfile, Segments};
    let target = Compiler::new(ExecutionProfile::Real16).target();
    let cpu = CpuState {
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    Machine::new(target, cpu, map, environment).unwrap()
}

fn ram() -> PhysicalMemoryMap {
    PhysicalMemoryMap::new([(0..=0x10ffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap()
}

async fn machine(code: &[u8]) -> TestMachine {
    let initial = real_machine(ram(), ());
    let mut machine = runtime().await.attach(initial).await.unwrap();
    machine.write_memory(0x100, code).unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    cpu.registers.esp = 0xff00;
    machine.set_cpu(cpu).unwrap();
    machine
}
