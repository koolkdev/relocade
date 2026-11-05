#[path = "../../machine/tests/support/clock.rs"]
mod clock;

use clock::ManualClock;
use relocade_frontend::keyboard::{ElementState, Key, KeyCode, KeyLocation, KeyboardEvent};
use relocade_machine::{worker::wake_connection, CpuStorage, Hook, Runner};
use relocade_pc::Pc;
use relocade_x86::{ExecutionExit, HostExit};
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn bios_without_dos() {
    let clock = ManualClock::default();
    let storage = CpuStorage::new().unwrap();
    let (_host, endpoint) = wake_connection(storage.interrupt_handle());
    let runner = Runner::new(clock.clone(), storage, endpoint);
    runner
        .run(
            futures_timer::Delay::new(Duration::from_secs(30)),
            async |runner| {
                let mut pc = Pc::new(runner).await.unwrap();
                let memory = pc.cpu().memory();
                // PC startup installs neither a DOS vector nor DOS allocation metadata.
                assert_eq!(memory.view().read_u32(0x21 * 4).unwrap(), 0);
                assert_eq!(memory.view().read_u8(0xfff0).unwrap(), 0);
                assert_eq!(memory.view().read_u32(0x46c).unwrap(), 0);

                // Query video, wait for a physical key through IRQ1, then read IRQ0's tick.
                let code = [
                    0xb4, 0x0f, 0xcd, 0x10, 0xa3, 0, 7, // Video mode.
                    0x31, 0xc0, 0xcd, 0x16, 0xa3, 2, 7, // Key.
                    0x83, 0x3e, 0x6c, 4, 0, 0x74, 0xf9, // Wait for IRQ0's BDA update.
                    0x31, 0xc0, 0xcd, 0x1a, 0x89, 0x16, 4, 7, // Clock.
                    0xeb, 0,
                ];
                memory.view().write(0x1000, &code).unwrap();
                let exit = pc.cpu().exit_handle();
                pc.cpu()
                    .install([Hook::host(0x1000 + code.len() as u32, async move || {
                        exit.request();
                    })])
                    .await
                    .unwrap();
                let state = pc.cpu().cpu_state();
                state.set_eip(0x1000);
                state.set_sp(0x8000);
                state.set_interrupts_enabled(true);
                pc.keyboard_input()
                    .unbounded_send(KeyboardEvent::Key {
                        physical_key: KeyCode::KeyA.into(),
                        logical_key: Key::Character("a".into()),
                        location: KeyLocation::Standard,
                        text: Some("a".into()),
                        state: ElementState::Pressed,
                        repeat: false,
                    })
                    .unwrap();
                clock.advance(Duration::from_nanos(54_925_402));
                assert_eq!(
                    runner.run_cpu(pc.cpu()).await,
                    ExecutionExit::Host(HostExit::Return)
                );
                assert_eq!(memory.view().read_u16(0x700).unwrap(), 0x5003);
                assert_eq!(memory.view().read_u16(0x702).unwrap(), 0x1e61);
                assert_eq!(memory.view().read_u16(0x704).unwrap(), 1);
                assert_eq!(memory.view().read_u32(0x21 * 4).unwrap(), 0);
            },
        )
        .await
        .expect("PC guest timed out");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pc_runs_bios_video_keyboard_and_timer_without_dos() {
    pollster::block_on(bios_without_dos());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pc_runs_bios_video_keyboard_and_timer_without_dos() {
    bios_without_dos().await;
}
