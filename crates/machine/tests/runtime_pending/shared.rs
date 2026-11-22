use super::WakeCount;
use futures_channel::oneshot;
use futures_util::FutureExt;
use relocade_machine::{Cpu, CpuConfig, Hook, MachineConfig, Memory};
use relocade_x86::{Compiler, CpuState, Exception, ExecutionExit, Registers, SegmentProfile};
use std::{
    future::Future,
    pin::pin,
    sync::{atomic::Ordering, Arc},
    task::{Context, Waker},
};

fn context(eip: u32, esp: u32, eax: u32) -> CpuConfig {
    let state = CpuState {
        eip,
        registers: Registers {
            esp,
            eax,
            ..Registers::default()
        },
        ..CpuState::default()
    };
    CpuConfig::new(Compiler::new(SegmentProfile::Flat32).target(), state).unwrap()
}

async fn check_shared_contexts() {
    const WAITER: u32 = 0x1_0100;
    const WRITER: u32 = 0x2_0100;
    let memory = Memory::new(MachineConfig::flat(0x3_0000).unwrap()).unwrap();
    let mut view = memory.view();
    view.write(
        WAITER,
        &[
            0x50, // PUSH EAX
            0xe8, 8, 0, 0, 0,    // CALL wait hook at WAITER+14
            0x5a, // POP EDX
            0xa1, 0, 4, 0, 0, // MOV EAX,[0400]
            0x0f, 0x0b, // UD2: report completion to the host
            0x0f, 0x0b, // UD2 if the hook was missed
            0xc3, // RET after the wait
        ],
    )
    .unwrap();
    view.write(
        WRITER,
        &[
            0x50, // PUSH EAX
            0xa3, 0, 4, 0, 0,    // MOV [0400],EAX
            0x5a, // POP EDX
            0x0f, 0x0b, // UD2
        ],
    )
    .unwrap();

    let mut waiter = Cpu::new_without_io(memory.clone(), context(WAITER, 0x1800, 0x11))
        .await
        .unwrap();
    let mut writer = Cpu::new_without_io(memory.clone(), context(WRITER, 0x2800, 0x22))
        .await
        .unwrap();
    let waiting_state = waiter.cpu_state();
    let (release, wait) = oneshot::channel::<()>();
    let wait = wait.shared();
    waiter
        .install([Hook::host(WAITER + 14, {
            let state = waiting_state.clone();
            async move || {
                wait.clone().await.unwrap();
                state.set_eip(WAITER + 16);
            }
        })])
        .await
        .unwrap();

    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut waiting = pin!(waiter.run());
    assert!(waiting
        .as_mut()
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    assert_eq!(waiting_state.eip(), WAITER + 14);
    assert_eq!(waiting_state.esp(), 0x17f8);
    assert_eq!(view.read_u32(0x17f8).unwrap(), WAITER + 6);

    let stop = ExecutionExit::Exception(Exception::InvalidOpcode);
    assert_eq!(writer.run().await, stop);
    assert_eq!(writer.cpu_state().eip(), WRITER + 7);
    assert_eq!(writer.cpu_state().esp(), 0x2800);
    assert_eq!(writer.cpu_state().edx(), 0x22);
    assert_eq!(view.read_u32(0x400).unwrap(), 0x22);
    assert_eq!(waiting_state.eax(), 0x11);

    let previous_wakes = wakes.0.load(Ordering::Relaxed);
    release.send(()).unwrap();
    assert!(wakes.0.load(Ordering::Relaxed) > previous_wakes);
    assert_eq!(waiting.await, stop);
    assert_eq!(waiting_state.eip(), WAITER + 12);
    assert_eq!(waiting_state.esp(), 0x1800);
    assert_eq!(waiting_state.edx(), 0x11); // recovered from the waiter's own stack
    assert_eq!(waiting_state.eax(), 0x22); // loaded from the writer's shared store
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_blocked_protected_context_allows_its_peer_to_run_on_shared_memory() {
    pollster::block_on(check_shared_contexts());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_blocked_protected_context_allows_its_peer_to_run_on_shared_memory() {
    check_shared_contexts().await;
}
