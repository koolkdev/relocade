use super::connect;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{Cpu, CpuConfig, DispatchEntry, MachineConfig, Memory};
use relocade_x86::{
    BlockSnapshot, Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap,
};

async fn guest_status_routing() {
    for compiled in [false, true] {
        let (_vga, io) = connect(&ManualClock::new());
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let memory = Memory::new(
            MachineConfig::new(
                0x1000,
                PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let code = [
            0xba, 0xc2, 0x03, 0xb0, 0x42, 0xee, // Select mono status.
            0xba, 0xba, 0x03, 0x66, 0xed, 0x66, 0x89, 0xc3, // IN EAX,DX; MOV EBX,EAX.
            0xba, 0xc2, 0x03, 0xb0, 0x43, 0xee, // Select color status.
            0xba, 0xba, 0x03, 0xec, 0x88, 0xc1, // Read the inactive mono alias into CL.
            0xba, 0xda, 0x03, 0xec, // Read active color status into AL.
            0xeb, 0, // End the compiled block at the following boundary.
        ];
        let instructions = [0, 3, 5, 6, 9, 11, 14, 17, 19, 20, 23, 24, 26, 29, 30];
        memory.view().write(0x100, &code).unwrap();
        let mut cpu = Cpu::new(
            memory,
            CpuConfig::new(
                compiler.target(),
                CpuState {
                    eip: 0x100,
                    ..CpuState::real_mode()
                },
            )
            .unwrap(),
            io,
        )
        .await
        .unwrap();
        if compiled {
            // I/O ends a compiled block. Install each instruction so execution
            // after an OUT continues through compiled port reads as well.
            let snapshots = instructions.map(|offset| BlockSnapshot {
                start_eip: 0x100 + offset as u32,
                bytes: &code[offset..],
                instruction_limit: 1,
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
        for _ in instructions {
            if compiled {
                let interrupt = cpu.interrupt_handle();
                interrupt.request();
                assert_eq!(cpu.run().await, ExecutionExit::Interrupted);
                interrupt.take();
            } else {
                assert_eq!(cpu.step(), ExecutionExit::StepComplete);
            }
        }
        assert_eq!(cpu.cpu_state().eip(), 0x100 + code.len() as u32);
        assert_eq!(cpu.cpu_state().ebx(), 0xffff_ff00);
        assert_eq!(cpu.cpu_state().cl(), 0xff);
        assert_eq!(cpu.cpu_state().al(), 0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_writes_replace_status_handlers_before_the_next_port_read() {
    pollster::block_on(guest_status_routing());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_writes_replace_status_handlers_before_the_next_port_read() {
    guest_status_routing().await;
}
