mod common;

use common::{complete, runtime};
use relocade_runtime::{
    compilation::{BackgroundCompiler, Compilation},
    BlockRequest, CodeSnapshot, Result,
};
use relocade_x86::ExecutionExit;
use std::num::NonZeroU16;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn worker_publication() {
    let mut runtime = runtime().await;
    assert!(runtime.has_compiler());
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0x00, 0x43, 0xeb, 0xfa])
        .unwrap();
    let snapshot = runtime
        .capture_connected_group(
            0x1000,
            NonZeroU16::new(8).unwrap(),
            NonZeroU16::new(16).unwrap(),
        )
        .unwrap();
    assert!(runtime.queue_group(snapshot).unwrap());
    runtime.set_compiler(None);
    assert!(!runtime.has_compiler());
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(runtime.run(20).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().ax(), 5);
    assert_eq!(runtime.machine().cpu().bx(), 5);
    runtime.machine().memory().write(0x5000, &[1]).unwrap();
    assert_eq!(complete(&mut runtime).await, 1);
    assert_eq!(runtime.compiled_group_count(), 1);
    assert_eq!(runtime.run(20).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().ax(), 10);
    assert_eq!(runtime.machine().cpu().bx(), 10);
}

async fn stale_completion() {
    let mut runtime = runtime().await;
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
    runtime.machine().memory().write(0x2000, &[0x48]).unwrap();
    assert_eq!(complete(&mut runtime).await, 0);
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().ax(), 0xffff);
}

struct HeldCompiler;
impl<M: 'static> BackgroundCompiler<M> for HeldCompiler {
    fn submit(&mut self, _snapshot: CodeSnapshot) -> Result<Option<Compilation<M>>> {
        Ok(Some(Box::pin(std::future::pending())))
    }
}

async fn queue_bounds() {
    let mut runtime = runtime().await;
    runtime.set_compiler(Some(Box::new(HeldCompiler)));
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    for (start_eip, accepted) in [
        (0x1000, true),
        (0x1000, false),
        (0x3000, true),
        (0x4000, false),
    ] {
        let snapshot = runtime
            .capture_group(&[BlockRequest {
                start_eip,
                instruction_limit: 1,
            }])
            .unwrap();
        assert_eq!(runtime.queue_group(snapshot).unwrap(), accepted);
    }
    assert_eq!(runtime.publish_ready_groups().await.unwrap(), 0);
    assert_eq!(runtime.pending_compilation_count(), 2);
    // Replacing the compiler cannot make accepted work disappear.
    runtime.set_compiler(Some(Box::new(HeldCompiler)));
    assert_eq!(
        runtime.run_async(20).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().ax(), 10);
    assert_eq!(runtime.pending_compilation_count(), 2);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn worker_compiles_owned_connected_code_for_private_guest_imports() {
    pollster::block_on(worker_publication());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn worker_compiles_owned_connected_code_for_private_guest_imports() {
    worker_publication().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn changed_code_cannot_publish_a_worker_result() {
    pollster::block_on(stale_completion());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn changed_code_cannot_publish_a_worker_result() {
    stale_completion().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_jobs_are_bounded_and_deduplicated_without_waiting() {
    pollster::block_on(queue_bounds());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pending_jobs_are_bounded_and_deduplicated_without_waiting() {
    queue_bounds().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn worker_startup_failure_releases_the_job_without_publication() {
    let mut runtime = runtime().await;
    runtime.set_compiler(Some(Box::new(
        relocade_runtime_web::WebCompiler::new(
            &common::test_url("/missing-compiler.js"),
            &common::test_url("/missing-compiler.wasm"),
        )
        .unwrap(),
    )));
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    let snapshot = runtime
        .capture_group(&[BlockRequest {
            start_eip: 0x1000,
            instruction_limit: 2,
        }])
        .unwrap();
    assert!(runtime.queue_group(snapshot).unwrap());
    loop {
        if runtime.publish_ready_groups().await.is_err() {
            break;
        }
        common::yield_host().await;
    }
    assert_eq!(runtime.pending_compilation_count(), 0);
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(runtime.machine().cpu().instruction_count(), 0);
}
