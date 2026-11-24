use super::connect;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{
    Cpu, CpuConfig, DispatchEntry, GuestTimeDriver, IoBus, MachineConfig, Memory,
};
use relocade_vga::DispiAdapter;
use relocade_x86::{
    BlockSnapshot, Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping,
    PhysicalMemoryMap,
};
use std::{cell::RefCell, num::NonZeroU64, rc::Rc, task::Waker};

async fn create_cpu(io: IoBus, code: &[u8], instruction_count: u64) -> Cpu {
    let memory = Memory::new(
        MachineConfig::new(
            0x1000,
            PhysicalMemoryMap::new([(0..=0xfff, PhysicalMapping::Ram { backing_offset: 0 })])
                .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    memory.view().write(0x100, code).unwrap();
    Cpu::new(
        memory,
        CpuConfig::new(
            Compiler::new(ExecutionProfile::Real16).config(),
            CpuState {
                eip: 0x100,
                instruction_count,
                ..CpuState::real_mode()
            },
        )
        .unwrap(),
        io,
    )
    .await
    .unwrap()
}

async fn guest_status_routing() {
    for compiled in [false, true] {
        let (_vga, io) = connect(&ManualClock::new());
        let compiler = Compiler::new(ExecutionProfile::Real16);
        let code = [
            0xba, 0xc2, 0x03, 0xb0, 0x42, 0xee, // Select mono status.
            0xba, 0xba, 0x03, 0x66, 0xed, 0x66, 0x89, 0xc3, // IN EAX,DX; MOV EBX,EAX.
            0xba, 0xc2, 0x03, 0xb0, 0x43, 0xee, // Select color status.
            0xba, 0xba, 0x03, 0xec, 0x88, 0xc1, // Read the inactive mono alias into CL.
            0xba, 0xda, 0x03, 0xec, // Read active color status into AL.
            0xeb, 0, // End the compiled block at the following boundary.
        ];
        let instructions = [0, 3, 5, 6, 9, 11, 14, 17, 19, 20, 23, 24, 26, 29, 30];
        let mut cpu = create_cpu(io, &code, 0).await;
        if compiled {
            // I/O ends a compiled block. Install each instruction so execution
            // after an OUT continues through compiled port reads as well.
            let code_default_size = cpu.cpu_state().code_default_size();
            let snapshots = instructions.map(|offset| BlockSnapshot {
                address: relocade_x86::CodeAddress::CsRelative(0x100 + offset as u32),
                code_default_size,
                bytes: &code[offset..],
                instruction_limit: 1,
            });
            let module = compiler.compile_blocks(&snapshots).unwrap();
            let entries: Vec<_> = module
                .entries
                .iter()
                .map(|entry| DispatchEntry {
                    key: entry.address.eip(),
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

async fn guest_status_deadlines() {
    let driver = GuestTimeDriver::new(NonZeroU64::new(1_000_000).unwrap());
    let vga = Rc::new(RefCell::new(DispiAdapter::with_clock(driver.clock())));
    let io = IoBus::new(vga.clone(), |io| DispiAdapter::configure_io(io, &vga));
    let mut cpu = create_cpu(io.clone(), &[0x90; 40], u64::MAX - 16).await;
    let _binding = driver.bind(&cpu, Waker::noop());
    assert_eq!(io.read_port(0x3da, 1), 0);
    for instructions in 1..=40 {
        assert_eq!(cpu.step(), ExecutionExit::StepComplete);
        // At one instruction per microsecond, the default VGA's 640-dot display
        // ends after 25.422 us and its 800-dot scanline ends after 31.778 us.
        let status = u32::from((26..32).contains(&instructions));
        assert_eq!(io.read_port(0x3da, 1), status, "instruction {instructions}");
        assert_eq!(io.read_port(0x3da, 1), status, "repeated observation");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn status_deadlines_follow_retirement_across_instruction_counter_wrap() {
    pollster::block_on(guest_status_deadlines());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn status_deadlines_follow_retirement_across_instruction_counter_wrap() {
    guest_status_deadlines().await;
}
