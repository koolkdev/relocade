#[path = "../../../tests/support/compilation.rs"]
mod compilation;

use super::*;
use crate::{CpuConfig, Error, Machine, MachineConfig};
use futures_channel::oneshot;
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMemoryMap, Segments};

fn cpu(compiler: &relocade_machine::CompilerService) -> (Machine, CpuHandle) {
    let mut machine =
        Machine::new(MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap()).unwrap();
    let cpu = machine
        .create_cpu(
            CpuConfig::new(
                Compiler::new(ExecutionProfile::Real16).target(),
                CpuState {
                    segments: Segments::real_mode(),
                    ..Default::default()
                },
            )
            .unwrap(),
            compiler,
        )
        .unwrap();
    (machine, cpu)
}

async fn failed_startup(compiler: &relocade_machine::CompilerService) {
    let (machine, cpu) = cpu(compiler);
    let worker = ExecutionWorker::<()>::spawn(
        cpu,
        async |_, _| Err(relocade_wasm_runtime::Error::new("construct CPU", "failed").into()),
        async |_| panic!("failed construction must not start the session"),
    );
    match worker.join().await.unwrap_err() {
        Error::Wasm(error) => assert_eq!(error.to_string(), "construct CPU: failed"),
        error => panic!("unexpected startup error: {error}"),
    }
    machine.shutdown().await.unwrap();
}

struct Dropped(Option<oneshot::Sender<()>>);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.take().unwrap().send(()).unwrap();
    }
}

async fn stopped_startup(compiler: &relocade_machine::CompilerService) {
    let (machine, cpu) = cpu(compiler);
    let (entered, started) = oneshot::channel();
    let (cleanup, mut cleaned) = oneshot::channel();
    let worker = ExecutionWorker::<()>::spawn(
        cpu,
        async move |_, _| {
            let _cleanup = Dropped(Some(cleanup));
            entered.send(()).unwrap();
            std::future::pending().await
        },
        async |_| panic!("cancelled construction must not start the session"),
    );
    started.await.unwrap();
    assert_eq!(worker.stop().await.unwrap(), None);
    assert_eq!(cleaned.try_recv().unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn construction_errors_reach_the_worker_owner() {
    pollster::block_on(compilation::run(failed_startup));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn construction_errors_reach_the_worker_owner() {
    compilation::run(failed_startup).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopping_during_construction_drops_startup_before_joining() {
    pollster::block_on(compilation::run(stopped_startup));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stopping_during_construction_drops_startup_before_joining() {
    compilation::run(stopped_startup).await;
}
