//! Run a small raw x86 program using host-defined output and exit ports.
//! These ports are this example's embedding protocol, not DOS/BIOS services.

use std::io::{self, Write};
use wasm86_machine::{CpuContext, Environment, Machine, MemoryAccess};
use wasm86_runtime::{Runtime, SliceStop};
use wasm86_runtime_wasmtime::WasmtimeBackend;
use wasm86_x86::{
    Compiler, CpuState, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap, Segments,
};

#[derive(Default)]
struct Console {
    output: Vec<u8>,
    exit_code: Option<u8>,
}

impl Environment for Console {
    fn write_port(&mut self, _: &mut MemoryAccess<'_>, port: u16, bytes: u8, value: u32) {
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
    let runtime = Runtime::new(WasmtimeBackend::new()?);
    let map = PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })])?;
    let context = CpuContext::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
    )?;
    let mut machine = runtime
        .attach(Machine::new(context, map, Console::default())?)
        .await?;
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
    let outcome = machine.run_slice_until(1000, |machine| {
        machine.with_environment(|environment| environment.exit_code.is_some())
    })?;
    machine.with_environment(|environment| io::stdout().write_all(&environment.output))?;
    if outcome.stop != SliceStop::HostRequested
        || machine.with_environment(|environment| environment.exit_code) != Some(0)
    {
        return Err(format!("guest stopped unexpectedly: {outcome:?}").into());
    }
    eprintln!("Guest exited after {} instructions.", outcome.completed);
    Ok(())
}
