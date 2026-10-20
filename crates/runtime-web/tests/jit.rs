mod common;

use common::{complete, runtime, HeldCompiler};
use relocade_runtime::{Hook, JitConfig};
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    num::{NonZeroU16, NonZeroU32, NonZeroU64},
    ops::ControlFlow,
    task::{Context, Waker},
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn one_job() -> JitConfig {
    JitConfig {
        hot_threshold: NonZeroU16::new(1).unwrap(),
        max_compilations: NonZeroU32::new(1).unwrap(),
        ..JitConfig::default()
    }
}

async fn connected_group() {
    let mut runtime = runtime().await;
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0, 0x43, 0xeb, 0xfa])
        .unwrap();
    let before = runtime.machine().cpu().snapshot();
    assert_eq!(
        runtime.run_async(0).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().snapshot(), before);
    assert_eq!(runtime.hot_threshold(), None);
    assert_eq!(runtime.jit_compilation_count(), 0);
    runtime
        .set_jit_config(JitConfig {
            hot_threshold: NonZeroU16::new(2).unwrap(),
            ..one_job()
        })
        .unwrap();
    assert_eq!(
        runtime.run_async(8).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().instruction_count(), 8);
    assert_eq!(runtime.machine().cpu().ax(), 2);
    assert_eq!(runtime.machine().cpu().bx(), 2);
    assert_eq!(runtime.jit_compilation_count(), 1);
    complete(&mut runtime).await;
    assert_eq!(runtime.compiled_group_count(), 1);
    assert_eq!(runtime.compiled_entry_count(), 2);
    assert_eq!(
        runtime.run_async(40).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().instruction_count(), 48);
    assert_eq!(runtime.machine().cpu().ax(), 12);
    assert_eq!(runtime.machine().cpu().bx(), 12);
    assert_eq!(runtime.machine().cpu().eip(), 0x1000);
    assert_eq!(runtime.jit_compilation_count(), 1);
}

async fn held_compilation() {
    let mut runtime = runtime().await;
    runtime.set_compiler(Some(Box::new(HeldCompiler)));
    runtime.set_jit_config(one_job()).unwrap();
    runtime.set_execution_slice(NonZeroU64::new(4).unwrap());
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    let mut running = Box::pin(runtime.run_async(100));
    assert!(running
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    drop(running);
    assert_eq!(runtime.machine().cpu().instruction_count(), 4);
    assert_eq!(runtime.machine().cpu().ax(), 2);
    assert_eq!(runtime.pending_compilation_count(), 1);
    assert_eq!(runtime.jit_compilation_count(), 1);
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(runtime.hot_threshold(), None);
    assert_eq!(
        runtime.run_async(0).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(
        runtime.run_async(6).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().instruction_count(), 10);
    assert_eq!(runtime.machine().cpu().ax(), 5);
    assert_eq!(runtime.pending_compilation_count(), 1);
    assert_eq!(runtime.jit_compilation_count(), 1);

    runtime
        .set_jit_config(JitConfig {
            max_compilations: NonZeroU32::new(5).unwrap(),
            ..one_job()
        })
        .unwrap();
    runtime.set_execution_slice(NonZeroU64::new(4096).unwrap());
    assert_eq!(
        runtime.run_async(4096).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    // Repeated requests after cooldown cannot duplicate the held job or consume
    // lifetime allowance, even after configuration permits more compilations.
    assert_eq!(runtime.machine().cpu().instruction_count(), 4106);
    assert_eq!(runtime.pending_compilation_count(), 1);
    assert_eq!(runtime.jit_compilation_count(), 1);
}

async fn compilation_and_deferred_calls() {
    let mut runtime = runtime().await;
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0x0d])
        .unwrap();
    runtime
        .machine()
        .memory()
        .write(0x1020, &[0x43, 0xeb, 0xdd])
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
    runtime.set_jit_config(one_job()).unwrap();
    runtime.set_execution_slice(NonZeroU64::new(2).unwrap());
    assert_eq!(
        runtime.run_async(8).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().instruction_count(), 8);
    assert_eq!(runtime.machine().cpu().ax(), 2);
    assert_eq!(runtime.machine().cpu().bx(), 2);
    assert_eq!(runtime.state().get(), 2);
    assert_eq!(runtime.machine().cpu().eip(), 0x1000);
    assert_eq!(runtime.jit_compilation_count(), 1);
    complete(&mut runtime).await;
    assert_eq!(runtime.compiled_entry_count(), 1); // The host hook ends discovery.
}

async fn alias_writes() {
    let mut runtime = runtime().await;
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    runtime.set_jit_config(one_job()).unwrap();
    assert_eq!(
        runtime.run_async(2).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().ax(), 1);
    // The job can be pending or already published. Both cases must observe the
    // new instruction through the writable alias before subsequent execution.
    runtime.machine().memory().write(0x2000, &[0x48]).unwrap();
    complete(&mut runtime).await;
    assert_eq!(runtime.compiled_group_count(), 0);
    assert_eq!(
        runtime.run_async(2).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().ax(), 0);
    assert_eq!(runtime.machine().cpu().instruction_count(), 4);
    assert_eq!(runtime.jit_compilation_count(), 1);
}

async fn limits_and_declined_requests() {
    let mut runtime = runtime().await;
    let mut config = JitConfig {
        hot_threshold: NonZeroU16::new(1).unwrap(),
        max_blocks: NonZeroU16::new(1).unwrap(),
        instructions_per_block: NonZeroU16::new(1).unwrap(),
        max_groups: NonZeroU16::new(2).unwrap(),
        max_compilations: NonZeroU32::new(1).unwrap(),
    };
    for (index, address) in [0x1000, 0x3000, 0x5000].into_iter().enumerate() {
        // Permit one new job per entry, independent of whether its INC-only
        // group is published before this run reaches the uncaptured JMP.
        config.max_compilations = NonZeroU32::new(index as u32 + 1).unwrap();
        runtime.set_jit_config(config).unwrap();
        runtime
            .machine()
            .memory()
            .write(address, &[0x40, 0xeb, 0xfd])
            .unwrap();
        runtime.machine().cpu().set_eip(address);
        assert_eq!(
            runtime.run_async(2).await.unwrap(),
            ExecutionExit::BudgetExhausted
        );
        complete(&mut runtime).await;
        assert_eq!(
            runtime.machine().cpu().instruction_count(),
            (index as u64 + 1) * 2
        );
        assert_eq!(runtime.machine().cpu().eip(), address);
        assert_eq!(runtime.compiled_group_count(), (index + 1).min(2));
        assert_eq!(runtime.compiled_entry_count(), (index + 1).min(2));
    }
    // Eviction released the first group's watch without retiring either survivor.
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    assert_eq!(runtime.compiled_group_count(), 2);
    runtime.machine().cpu().set_eip(0x1000);
    assert_eq!(
        runtime.run_async(4).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().ax(), 5);
    assert_eq!(runtime.machine().cpu().instruction_count(), 10);
    assert_eq!(runtime.jit_compilation_count(), 3);
    assert_eq!(runtime.hot_threshold(), None);
    config.max_groups = NonZeroU16::new(1).unwrap();
    runtime.set_jit_config(config).unwrap();
    assert_eq!(runtime.compiled_group_count(), 1);
    runtime.machine().memory().write(0x3000, &[0xf4]).unwrap();
    assert_eq!(runtime.compiled_group_count(), 1);
    assert_eq!(runtime.jit_compilation_count(), 3);

    // Unsupported work consumes its accepted job allowance, but cannot delay the
    // interpreter's ordinary exit or publish any entries.
    config.max_compilations = NonZeroU32::new(4).unwrap();
    runtime.set_jit_config(config).unwrap();
    runtime.machine().cpu().set_eip(0x3000);
    assert_eq!(
        runtime.run_async(10).await.unwrap(),
        ExecutionExit::UnsupportedInstruction {
            eip: 0x3000,
            opcode: 0xf4,
        }
    );
    complete(&mut runtime).await;
    assert_eq!(runtime.machine().cpu().instruction_count(), 10);
    assert_eq!(runtime.jit_compilation_count(), 4);
    assert_eq!(runtime.compiled_group_count(), 1);

    // A synchronous request cannot promote a different stopped CPU identity.
    config.max_compilations = NonZeroU32::new(5).unwrap();
    runtime.set_jit_config(config).unwrap();
    runtime
        .machine()
        .memory()
        .write(0x7000, &[0xeb, 0xfe])
        .unwrap();
    runtime.machine().cpu().set_eip(0x7000);
    assert_eq!(runtime.run(1).unwrap(), ExecutionExit::CompileRequest);
    runtime.machine().cpu().set_eip(0x5000);
    assert!(!runtime.queue_hot_request().unwrap());
    assert_eq!(runtime.jit_compilation_count(), 4);
}

async fn no_compiler() {
    let mut runtime = runtime().await;
    runtime.set_compiler(None);
    runtime.set_jit_config(one_job()).unwrap();
    runtime
        .machine()
        .memory()
        .write(0x1000, &[0x40, 0xeb, 0xfd])
        .unwrap();
    assert_eq!(
        runtime.run_async(64).await.unwrap(),
        ExecutionExit::BudgetExhausted
    );
    assert_eq!(runtime.machine().cpu().instruction_count(), 64);
    assert_eq!(runtime.machine().cpu().ax(), 32);
    assert_eq!(runtime.hot_threshold(), None);
    assert_eq!(runtime.jit_compilation_count(), 0);
    assert_eq!(runtime.compiled_group_count(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn hot_successors_share_a_module_and_preserve_the_original_budget() {
    pollster::block_on(connected_group());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn hot_successors_share_a_module_and_preserve_the_original_budget() {
    connected_group().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn automatic_compilation_does_not_block_and_survives_run_cancellation() {
    pollster::block_on(held_compilation());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn automatic_compilation_does_not_block_and_survives_run_cancellation() {
    held_compilation().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compilation_slices_and_deferred_calls_share_the_original_budget() {
    pollster::block_on(compilation_and_deferred_calls());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compilation_slices_and_deferred_calls_share_the_original_budget() {
    compilation_and_deferred_calls().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn automatic_groups_observe_alias_writes() {
    pollster::block_on(alias_writes());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn automatic_groups_observe_alias_writes() {
    alias_writes().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cache_limits_and_declined_requests_retain_interpreter_progress() {
    pollster::block_on(limits_and_declined_requests());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cache_limits_and_declined_requests_retain_interpreter_progress() {
    limits_and_declined_requests().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn missing_compiler_leaves_hot_entries_in_the_interpreter() {
    pollster::block_on(no_compiler());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn missing_compiler_leaves_hot_entries_in_the_interpreter() {
    no_compiler().await;
}
