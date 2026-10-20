mod common;

use common::{runtime, HeldCompiler};
use relocade_runtime::{BlockRequest, Hook};
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    num::NonZeroU64,
    ops::ControlFlow,
    task::{Context, Poll, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn held_compilation() {
    let mut runtime = runtime().await;
    runtime.set_compiler(Some(Box::new(HeldCompiler)));
    runtime.set_execution_slice(NonZeroU64::new(4).unwrap());
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    let snapshot = runtime
        .capture_group(&[BlockRequest {
            start_eip: 0x1000,
            instruction_limit: 16,
        }])
        .unwrap();
    assert!(runtime.queue_group(snapshot).unwrap());
    assert_eq!(
        runtime.run_async(0).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().instruction_count(), 0);
    let mut running = Box::pin(runtime.run_async(10));
    assert!(matches!(
        running
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    drop(running);
    assert_eq!(runtime.machine().cpu().instruction_count(), 4);
    assert_eq!(runtime.machine().cpu().ax(), 2);
    assert_eq!(runtime.pending_compilation_count(), 1);
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(
        runtime.run_async(6).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().instruction_count(), 10);
    assert_eq!(runtime.machine().cpu().ax(), 5);
    assert_eq!(runtime.pending_compilation_count(), 1);
}

async fn original_budget() {
    let mut runtime = runtime().await;
    runtime.set_execution_slice(NonZeroU64::new(2).unwrap());
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0x0d])
        .unwrap();
    runtime
        .machine()
        .memory()
        .write(0x1020, &[0x43, 0xe9, 0xdc, 0xff])
        .unwrap();
    runtime
        .install_hooks([Hook::<common::TestState>::new(0x1010, |call| {
            call.defer(async |call| {
                common::yield_host().await;
                call.state().set(call.state().get() + 1);
                call.cpu().set_eip(0x1020);
                ControlFlow::Continue(())
            })
        })])
        .await
        .unwrap();
    assert_eq!(
        runtime.run_async(10).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().instruction_count(), 10);
    assert_eq!(runtime.machine().cpu().ax(), 3);
    assert_eq!(runtime.machine().cpu().bx(), 2);
    assert_eq!(runtime.state().get(), 2);
    assert_eq!(runtime.machine().cpu().eip(), 0x1010);
}

async fn ready_host_calls() {
    let mut runtime = runtime().await;
    runtime.set_execution_slice(NonZeroU64::new(4).unwrap());
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0x0d])
        .unwrap();
    runtime
        .install_hooks([Hook::new(0x1010, |call| {
            call.defer(async |call| {
                call.cpu().set_eip(0x1000);
                ControlFlow::Continue(())
            })
        })])
        .await
        .unwrap();
    let mut running = Box::pin(runtime.run_async(10));
    assert!(running
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    drop(running);
    assert_eq!(runtime.machine().cpu().instruction_count(), 4);
    assert_eq!(runtime.machine().cpu().ax(), 2);
}

async fn collects_worker_results() {
    let mut runtime = runtime().await;
    runtime.set_execution_slice(NonZeroU64::new(256).unwrap());
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    let snapshot = runtime
        .capture_group(&[BlockRequest {
            start_eip: 0x1000,
            instruction_limit: 16,
        }])
        .unwrap();
    assert!(runtime.queue_group(snapshot).unwrap());
    for _ in 0..1000 {
        let count = runtime.machine().cpu().instruction_count();
        assert_eq!(
            runtime.run_async(65_536).await.unwrap(),
            ExecutionExit::BudgetExhausted
        );
        assert_eq!(runtime.machine().cpu().instruction_count(), count + 65_536);
        if runtime.compiled_group_count() != 0 {
            assert_eq!(runtime.pending_compilation_count(), 0);
            return;
        }
    }
    panic!("execution did not collect worker completion");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn guest_advances_while_compilation_is_held_and_survives_run_cancellation() {
    pollster::block_on(held_compilation());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn guest_advances_while_compilation_is_held_and_survives_run_cancellation() {
    held_compilation().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn slices_and_deferred_host_calls_share_the_original_budget() {
    pollster::block_on(original_budget());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn slices_and_deferred_host_calls_share_the_original_budget() {
    original_budget().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ready_host_calls_cannot_extend_the_execution_slice() {
    pollster::block_on(ready_host_calls());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn ready_host_calls_cannot_extend_the_execution_slice() {
    ready_host_calls().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn execution_collects_ready_worker_results_between_slices() {
    pollster::block_on(collects_worker_results());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn execution_collects_ready_worker_results_between_slices() {
    collects_worker_results().await;
}
