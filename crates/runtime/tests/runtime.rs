use std::sync::OnceLock;
use wasm86_runtime::{Machine, Runtime};
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};

#[path = "runtime/devices.rs"]
mod devices;
#[path = "runtime/execution.rs"]
mod execution;
#[path = "runtime/memory.rs"]
mod memory;

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().unwrap())
}

fn ram() -> PhysicalMemoryMap {
    PhysicalMemoryMap::new([(0..=0x10ffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap()
}

fn machine(code: &[u8]) -> Machine<()> {
    let mut machine = runtime().machine(ram(), ()).unwrap();
    machine.write_memory(0x100, code).unwrap();
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    cpu.registers.esp = 0xff00;
    machine.set_cpu(cpu).unwrap();
    machine
}
