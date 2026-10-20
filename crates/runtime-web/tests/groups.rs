use relocade_machine::MachineConfig;
use relocade_runtime::{BlockRequest, Hook, Runtime, RuntimeState};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segment, Segments,
};
use std::ops::ControlFlow;

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn runtime() -> Runtime<Backend<RuntimeState>> {
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x1000,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([
            (0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 }),
            (
                0x2000..=0x2fff,
                PhysicalMapping::Ram {
                    backing_offset: 0x1000,
                },
            ),
            (
                0x4000..=0x4fff,
                PhysicalMapping::Rom {
                    backing_offset: 0x1000,
                },
            ),
        ])
        .unwrap(),
    )
    .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let backend = Backend::with_state(RuntimeState::default()).unwrap();
    #[cfg(target_arch = "wasm32")]
    let backend = Backend::with_state(RuntimeState::default());
    Runtime::new(backend, config).await.unwrap()
}

async fn publish(runtime: &mut Runtime<Backend<RuntimeState>>, entries: &[u32]) {
    let requests: Vec<_> = entries
        .iter()
        .map(|&start_eip| BlockRequest {
            start_eip,
            instruction_limit: 16,
        })
        .collect();
    let group = runtime
        .prepare_group(&requests)
        .unwrap()
        .expect("captured group");
    runtime
        .install_group(group)
        .await
        .unwrap()
        .expect("published group");
}

async fn self_modification() {
    let mut runtime = runtime().await;
    // The watched store changes the successor's immediate through a RAM alias.
    runtime
        .machine()
        .memory()
        .write(
            0x1000,
            &[
                0xb8, 0x78, 0x56, 0x89, 0x07, 0xb8, 0x11, 0x11, 0xba, 0x34, 0x12, 0xeb, 0xf3,
            ],
        )
        .unwrap();
    runtime.machine().cpu().set_bx(0x2006);
    publish(&mut runtime, &[0x1000, 0x1005]).await;
    assert_eq!(runtime.compiled_group_count(), 1);
    assert_eq!(runtime.run(5).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().ax(), 0x5678);
    assert_eq!(runtime.machine().cpu().dx(), 0x1234);
    assert_eq!(runtime.machine().cpu().instruction_count(), 5);
    assert_eq!(runtime.compiled_group_count(), 0);
    // Reusing retired slots must not overwrite the interpreter fallback.
    publish(&mut runtime, &[0x1000, 0x1005]).await;
    assert_eq!(runtime.run(5).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.compiled_group_count(), 0);

    runtime
        .machine()
        .memory()
        .write(0x3000, &[0xb8, 1, 0, 0xeb, 0xfb])
        .unwrap();
    runtime.machine().cpu().set_eip(0x3000);
    publish(&mut runtime, &[0x3000]).await;
    runtime
        .machine()
        .memory()
        .write_backing(0x3001, &[2])
        .unwrap();
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().ax(), 2);
}

async fn admission_and_hooks() {
    let mut runtime = runtime().await;
    // External entry at the same linear address under a different CS must use
    // live EIP semantics: the compiled JMP target was captured relative to CS=0.
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    publish(&mut runtime, &[0x1000]).await;
    runtime.machine().cpu().set_segment(
        Segment::Cs,
        relocade_x86::SegmentRegister::real_mode(Segment::Cs, 0x100),
    );
    runtime.machine().cpu().set_eip(0);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().eip(), 0);
    assert_eq!(runtime.machine().cpu().ax(), 1);
    runtime
        .machine()
        .cpu()
        .set_segment(Segment::Cs, Segments::real_mode().cs);

    // A new boundary inside a captured block retires the entire group. The
    // interpreter must notice that boundary even when reached by fallthrough.
    runtime
        .machine()
        .memory()
        .write(0x3000, &[0x40, 0x43, 0xeb, 0xfc])
        .unwrap();
    runtime.machine().cpu().set_eip(0x3000);
    publish(&mut runtime, &[0x3000]).await;
    runtime
        .install_hooks([Hook::new(0x3001, |call| {
            call.memory().write(0x1000, &[0x48]).unwrap();
            ControlFlow::Break(ExecutionExit::HostReturn)
        })])
        .await
        .unwrap();
    assert_eq!(runtime.compiled_group_count(), 1);
    assert_eq!(runtime.run(10).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(runtime.machine().cpu().eip(), 0x3001);
    assert_eq!(runtime.machine().cpu().bx(), 0);
    assert_eq!(runtime.compiled_group_count(), 0);
    // Recompilation stops before the fixed entry and cannot replace it.
    publish(&mut runtime, &[0x3000]).await;
    runtime.machine().cpu().set_eip(0x3000);
    assert_eq!(runtime.run(10).unwrap(), ExecutionExit::HostReturn);
    assert!(runtime
        .prepare_group(&[BlockRequest {
            start_eip: 0x3001,
            instruction_limit: 1
        }])
        .unwrap()
        .is_none());
}

async fn stale_publication() {
    let mut runtime = runtime().await;
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    runtime
        .machine()
        .memory()
        .write(0x3000, &[0xc6, 0x06, 0, 0x10, 0x48, 0xeb, 0])
        .unwrap();
    let request = [BlockRequest {
        start_eip: 0x1000,
        instruction_limit: 16,
    }];
    let untracked = runtime.prepare_group(&request).unwrap().unwrap();
    runtime.machine().cpu().set_eip(0x3000);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert!(runtime.install_group(untracked).await.unwrap().is_none());
    runtime.machine().cpu().set_eip(0x1000);
    let group = runtime.prepare_group(&request).unwrap().unwrap();
    runtime.machine().memory().write(0x2000, &[0x48]).unwrap();
    assert!(runtime.install_group(group).await.unwrap().is_none());
    let group = runtime.prepare_group(&request).unwrap().unwrap();
    runtime
        .install_hooks([Hook::new(0x1000, |_| {
            ControlFlow::Break(ExecutionExit::HostReturn)
        })])
        .await
        .unwrap();
    assert!(runtime.install_group(group).await.unwrap().is_none());
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::HostReturn);
}

async fn shared_pages() {
    let mut runtime = runtime().await;
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    runtime
        .machine()
        .memory()
        .write(0x1100, &[0xbb, 0x11, 0, 0xeb, 0xfb])
        .unwrap();
    runtime
        .machine()
        .memory()
        .write(0x5000, &[0xc6, 0x06, 0x01, 0x21, 0x77, 0xe9, 0xf8, 0xf0])
        .unwrap();
    publish(&mut runtime, &[0x1000]).await;
    publish(&mut runtime, &[0x4100]).await;
    assert_eq!(runtime.compiled_group_count(), 2);
    runtime
        .install_hooks([Hook::new(0x1000, |_| {
            ControlFlow::Break(ExecutionExit::HostReturn)
        })])
        .await
        .unwrap();
    assert_eq!(runtime.compiled_group_count(), 1);
    runtime.machine().cpu().set_eip(0x5000);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().eip(), 0x4100);
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(runtime.run(2).unwrap(), ExecutionExit::BudgetExhausted);
    assert_eq!(runtime.machine().cpu().bx(), 0x77);
}

async fn deferred_writes() {
    let mut runtime = runtime().await;
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap(); // INC AX; JMP 1000
    publish(&mut runtime, &[0x1000]).await;
    runtime
        .install_hooks([Hook::new(0x3000, |call| {
            call.defer(async |call| {
                let mut waited = false;
                std::future::poll_fn(|context| {
                    if waited {
                        std::task::Poll::Ready(())
                    } else {
                        waited = true;
                        context.waker().wake_by_ref();
                        std::task::Poll::Pending
                    }
                })
                .await;
                call.memory().write(0x2000, &[0x48]).unwrap(); // DEC AX through an alias
                call.cpu().set_eip(0x1000);
                ControlFlow::Continue(())
            })
        })])
        .await
        .unwrap();
    runtime.machine().cpu().set_eip(0x3000);
    assert_eq!(
        runtime.run_async(2).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(runtime.machine().cpu().ax(), 0xffff);
    assert_eq!(runtime.machine().cpu().instruction_count(), 2);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn managed_groups_invalidate_before_guest_and_host_writes() {
    pollster::block_on(self_modification());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn managed_groups_invalidate_before_guest_and_host_writes() {
    self_modification().await;
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn entry_admission_and_new_hooks_preserve_execution_boundaries() {
    pollster::block_on(admission_and_hooks());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn entry_admission_and_new_hooks_preserve_execution_boundaries() {
    admission_and_hooks().await;
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stale_groups_cannot_replace_changed_code_or_hooks() {
    pollster::block_on(stale_publication());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stale_groups_cannot_replace_changed_code_or_hooks() {
    stale_publication().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_backing_stays_watched_until_all_groups_retire() {
    pollster::block_on(shared_pages());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn shared_backing_stays_watched_until_all_groups_retire() {
    shared_pages().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn deferred_host_writes_invalidate_managed_code_before_resuming() {
    pollster::block_on(deferred_writes());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn deferred_host_writes_invalidate_managed_code_before_resuming() {
    deferred_writes().await;
}
