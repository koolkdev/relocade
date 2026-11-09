//! Scoped target ownership across pending services.

use super::*;
use std::{
    future::Future,
    task::{Context, Waker},
};

async fn cancelled_binding() {
    use relocade_machine::Hook;
    use std::{cell::Cell, future::poll_fn, task::Poll};
    let driver = GuestTimeDriver::new(clock_origin(), NonZeroU64::new(1000).unwrap());
    let mut cpu = support::create_cpu(configuration(0)).await.unwrap();
    cpu.memory()
        .view()
        .write(0x100, &[0x40, 0xeb, 0x0d])
        .unwrap();
    cpu.memory()
        .view()
        .write(0x120, &[0x43, 0xeb, 0x0d])
        .unwrap();
    let state = cpu.cpu_state();
    let ready = Rc::new(Cell::new(false));
    let starts = Rc::new(Cell::new(0));
    cpu.install([
        Hook::host(0x110, {
            let (ready, starts, state) = (ready.clone(), starts.clone(), state.clone());
            async move || {
                starts.set(starts.get() + 1);
                poll_fn(|_| {
                    if ready.get() {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
                state.set_eip(0x120);
            }
        }),
        Hook::host(0x130, {
            let exit = cpu.exit_handle();
            async move || exit.request()
        }),
    ])
    .await
    .unwrap();
    let binding = driver.bind(&cpu, Waker::noop());
    driver.synchronize(Some(clock_origin() + Duration::from_millis(3)));
    let mut run = Box::pin(cpu.run());
    assert!(run
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert_eq!(state.instruction_count(), 2);
    drop(run);
    drop(binding);
    ready.set(true);
    // The saved service resumes once, and the old target at three cannot stop it.
    assert_eq!(
        cpu.run().await,
        ExecutionExit::Host(relocade_x86::HostExit::Return)
    );
    assert_eq!(state.instruction_count(), 4);
    assert_eq!(starts.get(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn releasing_the_binding_clears_its_target_and_preserves_the_pending_service() {
    pollster::block_on(cancelled_binding());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn releasing_the_binding_clears_its_target_and_preserves_the_pending_service() {
    cancelled_binding().await;
}
