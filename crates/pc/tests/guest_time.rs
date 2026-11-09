use relocade_machine::{DispatchEntry, ExecutionWorker, Hook};
use relocade_pc::Pc;
use relocade_x86::{Compiler, ExecutionExit, ExecutionProfile, HostExit};
use std::{num::NonZeroU64, time::Duration};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn pit_interrupts() {
    let worker = ExecutionWorker::spawn_with_guest_time(
        || relocade_machine::GuestTimeConfig::new(NonZeroU64::new(1_000_000).unwrap()),
        async |runner| {
            let start = runner.clock().now();
            let mut pc = Pc::new(runner).await.unwrap();
            let cpu = pc.cpu();
            let memory = cpu.memory();
            // Program Bumpy's mode-3 divisor 0951h, unmask IRQ0, then perform
            // guest work until eight interrupts have recorded its progress.
            let program = [
                0xb0, 0x36, 0xe6, 0x43, 0xb0, 0x51, 0xe6, 0x40, 0xb0, 0x09, 0xe6, 0x40, 0xb0, 0xfe,
                0xe6, 0x21, 0xfb, 0x90, 0xff, 0x06, 0x00, 0x07, // INC word [0700].
                0x83, 0x3e, 0x02, 0x07, 8, // CMP word [0702], 8.
                0x72, 0xf5, // JB to INC.
            ];
            memory.view().write(0x1000, &program).unwrap();
            memory.view().write(8 * 4, &[0, 0x20, 0, 0]).unwrap();
            // IRQ0: save AX, record main-loop progress, count the interrupt,
            // acknowledge the PIC, restore AX, and return to the busy loop.
            memory
                .view()
                .write(
                    0x2000,
                    &[
                        0x50, 0xa1, 0x00, 0x07, 0xab, 0xff, 0x06, 0x02, 0x07, 0xb0, 0x20, 0xe6,
                        0x20, 0x58, 0xcf,
                    ],
                )
                .unwrap();
            let state = cpu.cpu_state();
            state.set_eip(0x1000);
            state.set_sp(0x8000);
            state.set_di(0x600);
            let block = Compiler::new(ExecutionProfile::Real16)
                .compile_block(0x1012, &program[18..], 32)
                .unwrap();
            cpu.install_module(
                &block.bytes,
                &[DispatchEntry {
                    address: 0x1012,
                    export: &block.entry,
                }],
            )
            .await
            .unwrap();
            let stop = cpu.exit_handle();
            cpu.install([Hook::host(0x101d, async move || stop.request())])
                .await
                .unwrap();
            assert_eq!(
                runner.run_cpu(cpu).await,
                ExecutionExit::Host(HostExit::Return)
            );
            assert_eq!(memory.view().read_u16(0x702).unwrap(), 8);
            let samples = (0..8)
                .map(|index| memory.view().read_u16(0x600 + index * 2).unwrap())
                .collect::<Vec<_>>();
            assert!(samples[0] > 0);
            assert!(samples.windows(2).all(|pair| pair[0] < pair[1]));
            // PIT deadlines must drive eight approximately 2 ms periods
            // without an execution slice duration.
            assert!(runner.clock().now() >= start + Duration::from_millis(14));
            assert!(runner.clock().now() < start + Duration::from_millis(20));
        },
    );
    assert_eq!(worker.join().await.unwrap(), Some(()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pit_deadlines_interrupt_a_busy_pc_without_execution_slices() {
    pollster::block_on(pit_interrupts());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pit_deadlines_interrupt_a_busy_pc_without_execution_slices() {
    pit_interrupts().await;
}
