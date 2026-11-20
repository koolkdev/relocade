use relocade_frontend::keyboard::{
    ElementState, Key, KeyCode, KeyLocation, KeyboardEvent, NativeKey,
};
use relocade_machine::{Hook, Machine};
use relocade_pc::{configuration, Pc};
use relocade_x86::{ExecutionExit, HostExit};
use std::time::Duration;

const TIMER_WAIT: u32 = 0x1000;
const KEY_WAIT: u32 = 0x1200;
const IRQ1: u32 = 0x2000;
const SCANS: u16 = 0x700;

async fn exercise() {
    let mut machine = Machine::new(configuration::memory()).unwrap();
    let cpu = machine.create_cpu(configuration::cpu(), None).unwrap();
    let result = cpu
        .run_until(
            futures_timer::Delay::new(Duration::from_secs(30)),
            async |runner| {
                let mut pc = Pc::new(runner).await.unwrap();
                let keys = pc.keyboard_input();
                let cpu = pc.cpu();
                let memory = cpu.memory();
                // IRQ0 and IRQ1 enabled. HLT until CX BIOS timer ticks have elapsed.
                let timer_wait = [
                    0xb0, 0xfc, 0xe6, 0x21, // Unmask IRQ0 and IRQ1.
                    0x8b, 0x1e, 0x6c, 0x04, // MOV BX,[046Ch] (BIOS ticks).
                    0x01, 0xcb, // ADD BX,CX.
                    0xfb, 0xf4, // STI; HLT.
                    0x39, 0x1e, 0x6c, 0x04, // CMP [046Ch],BX.
                    0x72, 0xf9, // JB back to HLT.
                ];
                // Check DI against BX with interrupts disabled so a scan cannot
                // arrive between checking the buffer and sleeping for another IRQ.
                let key_wait = [
                    0xfa, 0xb0, 0xfd, 0xe6, 0x21, // CLI; only IRQ1 can wake HLT.
                    0x39, 0xdf, 0x73, 0x05, // CMP DI,BX; JAE to exit.
                    0xfb, 0xf4, 0xfa, // STI; HLT; CLI.
                    0xeb, 0xf7, // Jump back to CMP.
                ];
                let state = cpu.cpu_state();
                state.set_sp(0x8000);
                state.set_di(SCANS);
                memory.view().write(TIMER_WAIT, &timer_wait).unwrap();
                memory.view().write(KEY_WAIT, &key_wait).unwrap();
                memory.view().write_u32(9 * 4, IRQ1).unwrap();
                // Save AX, read port 60h into ES:DI, acknowledge IRQ1 and IRET.
                memory
                    .view()
                    .write(
                        IRQ1,
                        &[0x50, 0xe4, 0x60, 0xaa, 0xb0, 0x20, 0xe6, 0x20, 0x58, 0xcf],
                    )
                    .unwrap();
                let exit = cpu.exit_handle();
                cpu.install(
                    [
                        TIMER_WAIT + timer_wait.len() as u32,
                        KEY_WAIT + key_wait.len() as u32,
                    ]
                    .map(|address| {
                        let exit = exit.clone();
                        Hook::host(address, async move || exit.request())
                    }),
                )
                .await
                .unwrap();

                state.set_eip(TIMER_WAIT);
                state.set_cx(2);
                let returned = ExecutionExit::Host(HostExit::Return);
                assert_eq!(pc.run().await, returned);
                assert_eq!(state.di(), SCANS, "IRQ1 before the first physical key");
                assert!(memory.view().read_u16(0x46c).unwrap() >= 2);

                for (transition, bytes) in [(ElementState::Pressed, 2), (ElementState::Released, 4)]
                {
                    state.set_eip(KEY_WAIT);
                    state.set_bx(SCANS + bytes);
                    keys.unbounded_send(KeyboardEvent::Key {
                        physical_key: KeyCode::ArrowUp.into(),
                        logical_key: Key::Unidentified(NativeKey::Unidentified),
                        location: KeyLocation::Standard,
                        text: None,
                        state: transition,
                        repeat: false,
                    })
                    .unwrap();
                    assert_eq!(pc.run().await, returned);
                    assert_eq!(state.di(), SCANS + bytes);
                }
                let mut scans = [0; 4];
                memory.view().read(u32::from(SCANS), &mut scans).unwrap();
                assert_eq!(scans, [0xe0, 0x48, 0xe0, 0xc8]);

                // Twelve BIOS ticks outlast the default 500 ms typematic delay.
                state.set_eip(TIMER_WAIT);
                state.set_cx(12);
                assert_eq!(pc.run().await, returned);
                assert_eq!(state.di(), SCANS + 4, "IRQ1 repeated a released key");
            },
        )
        .await
        .unwrap();
    machine.shutdown().await.unwrap();
    assert_eq!(result, Some(()), "paced PC timed out");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn halted_pc_receives_pit_ticks_and_one_up_arrow_make_and_break() {
    pollster::block_on(exercise());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn halted_pc_receives_pit_ticks_and_one_up_arrow_make_and_break() {
    exercise().await;
}
