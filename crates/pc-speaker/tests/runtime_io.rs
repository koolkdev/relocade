use relocade_clock_test_support::ManualClock;
mod common;
#[path = "../../machine/tests/support/execution.rs"]
mod execution;

use relocade_machine::{Cpu, CpuConfig, DispatchEntry, MachineConfig, Memory};
use relocade_pc_speaker::{Speaker, PORT_RANGE};
use relocade_x86::{
    BlockSnapshot, Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap, Segments,
};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn guest_port_io() {
    for compiled in [false, true] {
        let clock = ManualClock::default();
        let (io, _speaker) = common::hardware(clock.clock());
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let memory = Memory::new(
            MachineConfig::new(
                4096,
                PhysicalMemoryMap::new([(0..=4095, PhysicalMapping::Ram { backing_offset: 0 })])
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let code = [
            0xb0, 0xb6, 0xe6, 0x43, // MOV AL,B6; OUT 43,AL
            0xb0, 5, 0xe6, 0x42, // MOV AL,5; OUT 42,AL
            0xb0, 0, 0xe6, 0x42, // MOV AL,0; OUT 42,AL
            0xb0, 3, 0xe6, 0x61, // MOV AL,3; OUT 61,AL
            0xe4, 0x61, // IN AL,61
            0xb0, 0x80, 0xe6, 0x43, // MOV AL,80; OUT 43,AL (count latch)
            0xe4, 0x42, // IN AL,42 (LSB)
            0x88, 0xc4, 0xe4, 0x42, // MOV AH,AL; IN AL,42 (MSB)
        ];
        memory.view().write(0x100, &code).unwrap();
        let config = CpuConfig::new(
            compiler.target(),
            CpuState {
                eip: 0x100,
                segments: Segments::real_mode(),
                ..CpuState::default()
            },
        )
        .unwrap();
        let mut cpu = Cpu::new(memory, config, io).await.unwrap();
        if compiled {
            let snapshots = [0, 4, 8, 12, 16, 18, 22, 24].map(|offset| BlockSnapshot {
                start_eip: 0x100 + offset as u32,
                bytes: &code[offset..],
                instruction_limit: 2,
            });
            let module = compiler.compile_blocks(&snapshots).unwrap();
            let entries: Vec<_> = module
                .entries
                .iter()
                .map(|entry| DispatchEntry {
                    address: entry.start_eip,
                    export: &entry.export,
                })
                .collect();
            cpu.install_module(&module.bytes, &entries).await.unwrap();
        }
        assert_eq!(
            execution::run_until(&mut cpu, 0x112).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(cpu.context().cpu.eax(), 0x23);
        assert_eq!(cpu.context().cpu.eip(), 0x112);
        clock.advance_to(clock.at(Duration::from_micros(5))); // Five PIT clocks: mode-3 count is 2.
        assert_eq!(
            execution::run_until(&mut cpu, 0x11c).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(cpu.context().cpu.eax(), 0x0200);
        assert_eq!(cpu.context().cpu.eip(), 0x11c);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn interpreted_and_compiled_speaker_io() {
    pollster::block_on(guest_port_io());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn interpreted_and_compiled_speaker_io() {
    guest_port_io().await;
}
