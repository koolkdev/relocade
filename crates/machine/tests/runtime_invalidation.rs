use support::Configuration;
mod support;

use relocade_machine::PagedMemoryMap;
use relocade_machine::{DispatchEntry, Error, Hook, Machine, RuntimeState};
use relocade_wasm_runtime::Backend;
use relocade_x86::{Compiler, CpuState, ExecutionExit, SegmentProfile};
use std::ops::ControlFlow;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

fn config() -> Configuration {
    let mut map = PagedMemoryMap::default();
    map.map(0..=0xfff, 0, true).unwrap();
    support::configuration(
        Compiler::new(SegmentProfile::Flat32).target(),
        CpuState {
            eip: 0x100,
            ..CpuState::default()
        },
        0x2000,
        map,
    )
    .unwrap()
}

async fn check_remapping() {
    let Configuration {
        cpu,
        machine: config,
    } = config();
    let mut backend = Backend::with_state(RuntimeState::new(())).unwrap();
    let space = Machine::new(&mut backend, config).unwrap();
    // Two different instructions at the same linear address after remapping.
    space
        .memory()
        .view()
        .write_backing(0x100, &[0xb8, 1, 0, 0, 0, 0xeb, 0xf9])
        .unwrap();
    space
        .memory()
        .view()
        .write_backing(0x1100, &[0xb8, 2, 0, 0, 0, 0xeb, 0xf9])
        .unwrap();
    let mut first = space.create_cpu(backend, cpu).await.unwrap();
    let backend = Backend::with_state(RuntimeState::new(())).unwrap();
    let mut second = space.create_cpu(backend, cpu).await.unwrap();
    let mut source = [0; 7];
    let version = space.memory().snapshot(0x100, &mut source).unwrap();
    let module = Compiler::new(SegmentProfile::Flat32)
        .compile_block(0x100, &source, 2)
        .unwrap();
    for runtime in [&mut first, &mut second] {
        runtime
            .install_module(
                &version,
                &module.bytes,
                &[DispatchEntry {
                    address: 0x100,
                    export: &module.entry,
                }],
            )
            .await
            .unwrap();
        assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
        assert_eq!(runtime.context().cpu().eax(), 1);
    }
    first
        .install_hooks([Hook::new(0x300, |call| {
            call.cpu().set_ecx(99);
            ControlFlow::Break(ExecutionExit::HostReturn)
        })])
        .await
        .unwrap();
    space
        .memory()
        .view()
        .map_pages(0..=0xfff, 0x1000, true)
        .unwrap();
    for runtime in [&mut first, &mut second] {
        assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
        assert_eq!(runtime.context().cpu().eax(), 2);
        // A fresh snapshot permits compiling and publishing the replacement.
        let version = space.memory().snapshot(0x100, &mut source).unwrap();
        let module = Compiler::new(SegmentProfile::Flat32)
            .compile_block(0x100, &source, 2)
            .unwrap();
        runtime
            .install_module(
                &version,
                &module.bytes,
                &[DispatchEntry {
                    address: 0x100,
                    export: &module.entry,
                }],
            )
            .await
            .unwrap();
        assert_eq!(runtime.run(1).unwrap(), ExecutionExit::BudgetExhausted);
        assert_eq!(runtime.context().cpu().eax(), 2);
    }
    first.context().cpu().set_eip(0x300);
    assert_eq!(first.run(1).unwrap(), ExecutionExit::HostReturn);
    assert_eq!(first.context().cpu().ecx(), 99);
}

async fn check_stale_installation() {
    let backend = Backend::with_state(RuntimeState::new(())).unwrap();
    let mut runtime = support::create_cpu(backend, config()).await.unwrap();
    let space = runtime.machine().clone();
    let version = space.memory().snapshot(0, &mut []).unwrap();
    let initializer = support::initializing_module(0x800, &[0x55]);
    let mut installation = Box::pin(runtime.install_module(&version, &initializer, &[]));
    #[cfg(target_arch = "wasm32")]
    {
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        // WebAssembly.compile is asynchronous. Remap after compilation starts,
        // while installation is waiting for compilation to finish.
        let mut context = Context::from_waker(Waker::noop());
        assert!(installation.as_mut().poll(&mut context).is_pending());
    }
    space
        .memory()
        .view()
        .map_pages(0..=0xfff, 0x1000, true)
        .unwrap();
    assert!(matches!(installation.as_mut().await, Err(Error::StaleCode)));
    drop(installation);
    let mut bytes = [0];
    space
        .memory()
        .view()
        .read_backing(0x800, &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0]);
    let version = space.memory().snapshot(0, &mut []).unwrap();
    runtime
        .install_module(&version, &initializer, &[])
        .await
        .unwrap();
    space
        .memory()
        .view()
        .read_backing(0x800, &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0x55]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn remapping_invalidates_generated_code_on_every_cpu_and_preserves_hooks() {
    pollster::block_on(check_remapping());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn remapping_invalidates_generated_code_on_every_cpu_and_preserves_hooks() {
    check_remapping().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stale_installation_is_rejected_before_import_initialization() {
    pollster::block_on(check_stale_installation());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stale_installation_is_rejected_before_import_initialization() {
    check_stale_installation().await;
}
