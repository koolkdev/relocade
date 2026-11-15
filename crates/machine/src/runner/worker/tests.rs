use super::*;
use crate::{CpuConfig, Error, Machine, MachineConfig};
use futures_channel::oneshot;
use relocade_x86::{Compiler, CpuState, ExecutionProfile, PhysicalMemoryMap};

fn cpu() -> (Machine, CpuHandle) {
    let mut machine =
        Machine::new(MachineConfig::new(0, PhysicalMemoryMap::new([]).unwrap()).unwrap()).unwrap();
    let cpu = machine
        .create_cpu(
            CpuConfig::new(
                Compiler::new(ExecutionProfile::Real16).target(),
                CpuState::real_mode(),
            )
            .unwrap(),
            None,
        )
        .unwrap();
    (machine, cpu)
}

async fn failed_startup() {
    let (machine, cpu) = cpu();
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

async fn stopped_startup() {
    let (machine, cpu) = cpu();
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

#[derive(Default)]
struct Gate {
    open: std::sync::Mutex<bool>,
    changed: std::sync::Condvar,
}

impl Gate {
    fn wait(&self) {
        drop(
            self.changed
                .wait_while(self.open.lock().unwrap(), |open| !*open)
                .unwrap(),
        );
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

struct Release(std::sync::Arc<Gate>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

async fn jobs_accepted_during_startup_are_drained() {
    let (machine, cpu) = cpu();
    let (completed, completion) = oneshot::channel();
    let worker = cpu.spawn_with_guest_time(
        || crate::GuestTimeConfig::new(std::num::NonZeroU64::new(1000).unwrap()),
        async move |runner| {
            let jobs = runner.spawn_job_thread();
            let mut work = std::pin::pin!(jobs.execute(move || completed.send(()).unwrap()));
            // Submit once, then finish the session without waiting for worker startup.
            let _ = futures_lite::future::poll_once(work.as_mut()).await;
        },
    );
    worker.join().await.unwrap();
    completion.await.unwrap();
    machine.shutdown().await.unwrap();
}

async fn owned_job_cleanup(cancel: bool) {
    let (machine, cpu) = cpu();
    let (export, exported) = oneshot::channel();
    let (started, ready) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let (cleanup, mut cleaned) = oneshot::channel();
    let gate = std::sync::Arc::new(Gate::default());
    let release = Release(gate.clone());
    let worker = cpu.spawn_with_guest_time(
        || crate::GuestTimeConfig::new(std::num::NonZeroU64::new(1000).unwrap()),
        async move |runner| {
            let jobs = runner.spawn_job_thread();
            let mut work = std::pin::pin!(jobs.execute(move || {
                let _cleanup = Dropped(Some(cleanup));
                started.send(()).unwrap();
                gate.wait();
            }));
            assert!(futures_lite::future::poll_once(work.as_mut())
                .await
                .is_none());
            export
                .send(jobs.clone())
                .unwrap_or_else(|_| panic!("job owner"));
            let _ = finished.await;
        },
    );
    let jobs = exported.await.unwrap();
    ready.await.unwrap();
    if !cancel {
        finish.send(()).unwrap();
    }
    let mut shutdown = std::pin::pin!(async move {
        if cancel {
            worker.stop().await
        } else {
            worker.join().await
        }
    });
    assert!(futures_lite::future::poll_once(shutdown.as_mut())
        .await
        .is_none());
    assert_eq!(cleaned.try_recv().unwrap(), None);
    drop(release);
    assert_eq!(shutdown.await.unwrap(), (!cancel).then_some(()));
    assert_eq!(cleaned.try_recv().unwrap(), Some(()));
    assert!(matches!(
        jobs.execute(|| ()).await,
        Err(crate::worker::PoolError::Closed)
    ));
    machine.shutdown().await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn construction_errors_reach_the_worker_owner() {
    pollster::block_on(failed_startup());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn construction_errors_reach_the_worker_owner() {
    failed_startup().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopping_during_construction_drops_startup_before_joining() {
    pollster::block_on(stopped_startup());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stopping_during_construction_drops_startup_before_joining() {
    stopped_startup().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn session_completion_and_cancellation_drain_calculation_jobs() {
    pollster::block_on(async {
        jobs_accepted_during_startup_are_drained().await;
        owned_job_cleanup(false).await;
        owned_job_cleanup(true).await;
    });
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn session_completion_and_cancellation_drain_calculation_jobs() {
    jobs_accepted_during_startup_are_drained().await;
    owned_job_cleanup(false).await;
    owned_job_cleanup(true).await;
}
