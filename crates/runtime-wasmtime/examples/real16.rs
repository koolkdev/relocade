//! Run a small raw x86 program using host-defined output and exit ports.
//! These ports are this example's embedding protocol, not DOS/BIOS services.

use std::io::{self, Write};
use wasm86_runtime::{DeviceMemory, Devices, RunStop, Runtime};
use wasm86_runtime_wasmtime::WasmtimeBackend;
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};

#[derive(Default)]
struct Console {
    output: Vec<u8>,
    exit_code: Option<u8>,
}

impl Devices for Console {
    fn write_port(&mut self, _: &mut DeviceMemory<'_>, port: u16, bytes: u8, value: u32) {
        if bytes == 1 {
            match port {
                0xe9 => self.output.push(value as u8),
                0xf4 => self.exit_code = Some(value as u8),
                _ => {}
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pollster::block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new(WasmtimeBackend::new()?).await?;
    let map = PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])?;
    let mut machine = runtime.machine(map, Console::default()).await?;
    machine.write_memory(
        0x100,
        &[
            0xb0, b'H', 0xe6, 0xe9, // MOV AL,'H'; OUT E9,AL
            0xb0, b'i', 0xe6, 0xe9, 0xb0, b'\n', 0xe6, 0xe9, 0xb0, 0, 0xe6,
            0xf4, // Request exit with code zero.
            0x0f, 0x0b, // UD2 must not execute after the exit request.
        ],
    )?;
    let mut cpu = machine.cpu();
    cpu.eip = 0x100;
    cpu.registers.esp = 0xfffe;
    machine.set_cpu(cpu)?;
    let outcome = machine.run(1000, |machine| {
        machine.with_devices(|devices| devices.exit_code.is_some())
    })?;
    machine.with_devices(|devices| io::stdout().write_all(&devices.output))?;
    if outcome.stop != RunStop::HostRequested
        || machine.with_devices(|devices| devices.exit_code) != Some(0)
    {
        return Err(format!("guest stopped unexpectedly: {outcome:?}").into());
    }
    eprintln!("Guest exited after {} instructions.", outcome.completed);
    Ok(())
}
